//! Decoded audio samples, shared by the editor, the analysis pass and playback.

use std::path::Path;

/// Interleaved-free audio storage: one `Vec<f32>` per channel.
///
/// Sample values are normalized to `-1.0..=1.0` regardless of the file's bit
/// depth, mirroring the browser `AudioBuffer` the original frontend worked with.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBuffer {
    sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: Vec<Vec<f32>>) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// Builds a buffer with `frames` silent samples per channel.
    pub fn silent(sample_rate: u32, channel_count: usize, frames: usize) -> Self {
        Self {
            sample_rate,
            channels: vec![vec![0.0; frames]; channel_count.max(1)],
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn channels(&self) -> &[Vec<f32>] {
        &self.channels
    }

    pub fn channel(&self, index: usize) -> &[f32] {
        &self.channels[index]
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map(Vec::len).unwrap_or(0)
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / self.sample_rate as f64
    }

    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    /// Decodes a WAV file, which is the only format the engine produces:
    /// imports are normalized to 24-bit PCM WAV and recordings are written as
    /// 32-bit float WAV before normalization.
    pub fn decode_wav(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let mut reader = hound::WavReader::open(path)
            .map_err(|error| format!("Unable to open {}: {error}", path.display()))?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err("The audio file has no readable channels or sample rate.".into());
        }
        let frames = reader.len() as usize / spec.channels as usize;
        let mut channels = vec![Vec::with_capacity(frames); spec.channels as usize];
        match spec.sample_format {
            hound::SampleFormat::Float => {
                for (index, sample) in reader.samples::<f32>().enumerate() {
                    let sample = sample
                        .map_err(|error| format!("Unable to decode {}: {error}", path.display()))?;
                    channels[index % spec.channels as usize].push(sample.clamp(-1.0, 1.0));
                }
            }
            hound::SampleFormat::Int => {
                let scale = match spec.bits_per_sample {
                    8 => i8::MAX as f32,
                    16 => i16::MAX as f32,
                    24 => 8_388_607.0,
                    bits => (1_i64 << (bits.saturating_sub(1))) as f32,
                };
                for (index, sample) in reader.samples::<i32>().enumerate() {
                    let sample = sample
                        .map_err(|error| format!("Unable to decode {}: {error}", path.display()))?;
                    channels[index % spec.channels as usize]
                        .push((sample as f32 / scale).clamp(-1.0, 1.0));
                }
            }
        }
        Ok(Self::new(spec.sample_rate, channels))
    }

    pub fn slice_seconds(&self, start: f64, end: f64) -> Self {
        let start = self.frame_at(start);
        let end = self.frame_at(end).max(start).min(self.frames());
        Self::new(
            self.sample_rate,
            self.channels
                .iter()
                .map(|channel| channel[start..end].to_vec())
                .collect(),
        )
    }

    pub fn frame_at(&self, seconds: f64) -> usize {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        ((seconds * self.sample_rate as f64).floor() as usize).min(self.frames())
    }

    /// Downmixes every channel into a single mono track, used by the playback
    /// engine when the output device exposes fewer channels than the media.
    pub fn mono(&self) -> Vec<f32> {
        if self.channels.len() == 1 {
            return self.channels[0].clone();
        }
        let mut mixed = vec![0.0; self.frames()];
        for channel in &self.channels {
            for (index, sample) in channel.iter().enumerate() {
                mixed[index] += *sample;
            }
        }
        let gain = 1.0 / self.channels.len() as f32;
        for sample in &mut mixed {
            *sample *= gain;
        }
        mixed
    }
}

#[cfg(test)]
mod tests {
    use super::AudioBuffer;

    #[test]
    fn reports_duration_from_frames_and_rate() {
        let buffer = AudioBuffer::new(48_000, vec![vec![0.0; 24_000]]);
        assert_eq!(buffer.duration(), 0.5);
        assert_eq!(buffer.frames(), 24_000);
    }

    #[test]
    fn downmixes_multichannel_audio() {
        let buffer = AudioBuffer::new(8_000, vec![vec![1.0, -1.0], vec![0.0, 0.5]]);
        assert_eq!(buffer.mono(), vec![0.5, -0.25]);
    }

    #[test]
    fn clamps_bad_offsets_to_the_available_audio() {
        let buffer = AudioBuffer::new(1_000, vec![vec![0.0; 100]]);
        assert_eq!(buffer.frame_at(-1.0), 0);
        assert_eq!(buffer.frame_at(1.0), 100);
        assert_eq!(buffer.slice_seconds(-0.5, 0.03).frames(), 30);
    }
}
