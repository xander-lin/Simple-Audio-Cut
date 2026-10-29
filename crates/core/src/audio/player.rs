//! Playback output behind a small, mockable interface.
//!
//! The editor never talks to an audio backend directly; it starts, stops and
//! queries an [`AudioPlayer`]. The production implementation streams through
//! CPAL on a dedicated thread, while [`MockAudioPlayer`] advances a virtual
//! clock so UI tests can exercise transport behaviour without any audio device.

use crate::buffer::AudioBuffer;
use std::sync::{Arc, Mutex};

/// Plays a single decoded buffer at a time.
pub trait AudioPlayer: Send + Sync {
    /// Starts playback at `offset` seconds of `buffer`, replacing any current
    /// playback.
    fn play(&self, buffer: Arc<AudioBuffer>, offset: f64) -> Result<(), String>;

    /// Stops playback. Safe to call when nothing is playing.
    fn stop(&self);

    /// Position in seconds inside the played buffer, if something is playing.
    fn position(&self) -> Option<f64>;

    fn is_playing(&self) -> bool {
        self.position().is_some()
    }
}

struct VirtualPlayback {
    position: f64,
    playing: bool,
    calls: Vec<String>,
}

/// A player with no audio device. Positions only advance through
/// [`MockAudioPlayer::advance`], which keeps tests deterministic.
pub struct MockAudioPlayer {
    state: Mutex<VirtualPlayback>,
}

impl MockAudioPlayer {
    pub fn deterministic() -> Self {
        Self {
            state: Mutex::new(VirtualPlayback {
                position: 0.0,
                playing: false,
                calls: Vec::new(),
            }),
        }
    }

    /// Moves the simulated playhead forward.
    pub fn advance(&self, seconds: f64) {
        if let Ok(mut state) = self.state.lock() {
            if state.playing {
                state.position += seconds.max(0.0);
            }
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| state.calls.clone())
            .unwrap_or_default()
    }

    pub fn make_playing(&self, position: f64) {
        if let Ok(mut state) = self.state.lock() {
            state.playing = true;
            state.position = position;
        }
    }
}

impl Default for MockAudioPlayer {
    fn default() -> Self {
        Self::deterministic()
    }
}

impl AudioPlayer for MockAudioPlayer {
    fn play(&self, buffer: Arc<AudioBuffer>, offset: f64) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Mock playback state is unavailable.".to_string())?;
        state.calls.push(format!(
            "play {} Hz @{offset:.3}",
            buffer.sample_rate()
        ));
        state.position = offset.max(0.0);
        state.playing = true;
        Ok(())
    }

    fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.calls.push("stop".into());
            state.playing = false;
        }
    }

    fn position(&self) -> Option<f64> {
        let state = self.state.lock().ok()?;
        state.playing.then_some(state.position)
    }
}

#[cfg(feature = "native-audio")]
mod output {
    //! CPAL output.
    //!
    //! `cpal::Stream` is not `Send` on every backend, so the stream is created,
    //! driven and dropped on a dedicated thread that the player talks to over a
    //! command channel. The playhead is the only shared mutable state.

    use super::{AudioBuffer, AudioPlayer};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream, StreamConfig};
    use std::sync::mpsc::{Sender, SyncSender};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    struct StreamState {
        buffer: Arc<AudioBuffer>,
        /// Fractional read position in buffer frames.
        cursor: f64,
        /// Buffer frames consumed per output frame.
        ratio: f64,
    }

    impl StreamState {
        fn next_frame(&mut self, output_channels: usize) -> Vec<f32> {
            let frames = self.buffer.frames();
            if frames == 0 || self.cursor >= frames as f64 {
                return vec![0.0; output_channels];
            }
            let index = (self.cursor as usize).min(frames - 1);
            let next = (index + 1).min(frames - 1);
            let fraction = self.cursor - index as f64;
            let mut frame = vec![0.0; output_channels];
            for (channel, value) in frame.iter_mut().enumerate() {
                let source = channel.min(self.buffer.channel_count().saturating_sub(1));
                let samples = self.buffer.channel(source);
                let a = samples[index] as f64;
                let b = samples[next] as f64;
                *value = (a + (b - a) * fraction) as f32;
            }
            self.cursor += self.ratio;
            frame
        }
    }

    enum Command {
        Play {
            buffer: Arc<AudioBuffer>,
            offset: f64,
            reply: SyncSender<Result<(), String>>,
        },
        Stop {
            reply: SyncSender<()>,
        },
    }

    /// Streams decoded audio to the default output device with CPAL.
    pub struct CpalAudioPlayer {
        commands: Mutex<Option<Sender<Command>>>,
        thread: Mutex<Option<JoinHandle<()>>>,
        state: Arc<Mutex<Option<StreamState>>>,
    }

    impl CpalAudioPlayer {
        pub fn new() -> Self {
            Self {
                commands: Mutex::new(None),
                thread: Mutex::new(None),
                state: Arc::new(Mutex::new(None)),
            }
        }

        fn ensure_thread(&self) -> Result<Sender<Command>, String> {
            let mut commands = self
                .commands
                .lock()
                .map_err(|_| "Audio output commands are unavailable.".to_string())?;
            if let Some(sender) = commands.as_ref() {
                return Ok(sender.clone());
            }
            let (sender, receiver) = std::sync::mpsc::channel::<Command>();
            let state = self.state.clone();
            let handle = std::thread::Builder::new()
                .name("simple-audio-cut-output".into())
                .spawn(move || output_loop(receiver, state))
                .map_err(|error| format!("Unable to start the audio output thread: {error}"))?;
            *commands = Some(sender.clone());
            if let Ok(mut thread) = self.thread.lock() {
                *thread = Some(handle);
            }
            Ok(sender)
        }
    }

    impl Default for CpalAudioPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for CpalAudioPlayer {
        fn drop(&mut self) {
            self.stop();
            if let Ok(mut commands) = self.commands.lock() {
                *commands = None;
            }
            if let Ok(mut thread) = self.thread.lock() {
                if let Some(handle) = thread.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    impl AudioPlayer for CpalAudioPlayer {
        fn play(&self, buffer: Arc<AudioBuffer>, offset: f64) -> Result<(), String> {
            if buffer.is_empty() {
                return Err("The edited audio is empty.".into());
            }
            let sender = self.ensure_thread()?;
            let (reply, response) = std::sync::mpsc::sync_channel(1);
            sender
                .send(Command::Play {
                    buffer,
                    offset,
                    reply,
                })
                .map_err(|_| "The audio output thread stopped unexpectedly.".to_string())?;
            response
                .recv()
                .map_err(|_| "The audio output thread stopped unexpectedly.".to_string())?
        }

        fn stop(&self) {
            let sender = match self.commands.lock() {
                Ok(commands) => commands.clone(),
                Err(_) => None,
            };
            if let Some(sender) = sender {
                let (reply, response) = std::sync::mpsc::sync_channel(1);
                if sender.send(Command::Stop { reply }).is_ok() {
                    let _ = response.recv();
                }
            }
            if let Ok(mut state) = self.state.lock() {
                *state = None;
            }
        }

        fn position(&self) -> Option<f64> {
            let guard = self.state.lock().ok()?;
            let state = guard.as_ref()?;
            if state.cursor >= state.buffer.frames() as f64 {
                return None;
            }
            Some(state.cursor / state.buffer.sample_rate() as f64)
        }
    }

    fn output_loop(
        receiver: std::sync::mpsc::Receiver<Command>,
        state: Arc<Mutex<Option<StreamState>>>,
    ) {
        let mut stream: Option<Stream> = None;
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Play {
                    buffer,
                    offset,
                    reply,
                } => {
                    stream = None;
                    match start_stream(buffer, offset, state.clone()) {
                        Ok(started) => {
                            stream = Some(started);
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            if let Ok(mut guard) = state.lock() {
                                *guard = None;
                            }
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                Command::Stop { reply } => {
                    stream = None;
                    if let Ok(mut guard) = state.lock() {
                        *guard = None;
                    }
                    let _ = reply.send(());
                }
            }
        }
        drop(stream);
    }

    fn start_stream(
        buffer: Arc<AudioBuffer>,
        offset: f64,
        state: Arc<Mutex<Option<StreamState>>>,
    ) -> Result<Stream, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No default audio output device was found.")?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Unable to read output settings: {error}"))?;
        let config: StreamConfig = supported.config();
        let output_channels = config.channels as usize;
        let ratio = buffer.sample_rate() as f64 / config.sample_rate.0 as f64;
        let cursor = offset.max(0.0) * buffer.sample_rate() as f64;
        if let Ok(mut guard) = state.lock() {
            *guard = Some(StreamState {
                buffer: buffer.clone(),
                cursor,
                ratio,
            });
        }
        let callback_state = state.clone();
        let on_error = |error| eprintln!("Audio output stream error: {error}");
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    if let Ok(mut guard) = callback_state.try_lock() {
                        if let Some(state) = guard.as_mut() {
                            for frame in data.chunks_mut(output_channels) {
                                let next = state.next_frame(output_channels);
                                frame.copy_from_slice(&next);
                            }
                        }
                    }
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    if let Ok(mut guard) = callback_state.try_lock() {
                        if let Some(state) = guard.as_mut() {
                            for frame in data.chunks_mut(output_channels) {
                                let next = state.next_frame(output_channels);
                                for (sample, value) in frame.iter_mut().zip(next) {
                                    *sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                                }
                            }
                        }
                    }
                },
                on_error,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    if let Ok(mut guard) = callback_state.try_lock() {
                        if let Some(state) = guard.as_mut() {
                            for frame in data.chunks_mut(output_channels) {
                                let next = state.next_frame(output_channels);
                                for (sample, value) in frame.iter_mut().zip(next) {
                                    let scaled = (value.clamp(-1.0, 1.0) + 1.0) * 0.5;
                                    *sample = (scaled * u16::MAX as f32) as u16;
                                }
                            }
                        }
                    }
                },
                on_error,
                None,
            ),
            format => return Err(format!("Unsupported output sample format: {format:?}")),
        }
        .map_err(|error| format!("Unable to open the audio output: {error}"))?;
        stream
            .play()
            .map_err(|error| format!("Unable to start audio playback: {error}"))?;
        Ok(stream)
    }
}

#[cfg(feature = "native-audio")]
pub use output::CpalAudioPlayer;

#[cfg(test)]
mod tests {
    use super::{AudioPlayer, MockAudioPlayer};
    use crate::buffer::AudioBuffer;
    use std::sync::Arc;

    #[test]
    fn mock_player_tracks_position_and_stop() {
        let player = MockAudioPlayer::deterministic();
        assert!(!player.is_playing());
        player
            .play(
                Arc::new(AudioBuffer::new(1_000, vec![vec![0.0; 1_000]])),
                0.25,
            )
            .expect("play");
        assert_eq!(player.position(), Some(0.25));
        player.advance(0.5);
        assert_eq!(player.position(), Some(0.75));
        player.stop();
        assert_eq!(player.position(), None);
        assert_eq!(player.calls().len(), 2);
    }
}
