use super::{AudioEngine, DenoiseCompletion, DenoiseResult, EnvelopePoint, RecordingInfo, Region};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct ActiveRecording {
    stop_tx: std::sync::mpsc::Sender<()>,
    worker: std::thread::JoinHandle<Result<(), String>>,
    samples: Arc<Mutex<Vec<f32>>>,
    config: StreamConfig,
}

pub struct NativeAudioEngine {
    recording: Mutex<Option<ActiveRecording>>,
    output_dir: PathBuf,
}

impl NativeAudioEngine {
    pub fn new(output_dir: PathBuf) -> Result<Self, String> {
        ffmpeg_next::init().map_err(|error| format!("Unable to initialize FFmpeg: {error}"))?;
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("Unable to create recording directory: {error}"))?;
        Ok(Self {
            recording: Mutex::new(None),
            output_dir,
        })
    }

    fn clearvoice_runtime(&self) -> Result<(PathBuf, PathBuf), String> {
        let data_dir = self
            .output_dir
            .parent()
            .ok_or("Unable to locate the application data directory.")?;

        let mut python_candidates = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON")
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        if let Ok(current_dir) = std::env::current_dir() {
            python_candidates.push(current_dir.join(".venv/bin/python"));
        }
        python_candidates.push(data_dir.join("clearvoice/.venv/bin/python"));
        let python = python_candidates.into_iter().find(|path| path.is_file());

        let mut worker_candidates = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER")
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        if let Ok(current_dir) = std::env::current_dir() {
            worker_candidates.push(current_dir.join("tools/clearvoice_denoise.py"));
        }
        worker_candidates.push(PathBuf::from(
            "/usr/lib/simple-audio-cut/clearvoice_denoise.py",
        ));
        let worker = worker_candidates.into_iter().find(|path| path.is_file());

        match (python, worker) {
            (Some(python), Some(worker)) => Ok((python, worker)),
            _ => Err(
                "ClearVoice is not installed. Run `simple-audio-cut-clearvoice-setup`, or create the source-tree `.venv` documented in README.md."
                    .into(),
            ),
        }
    }

    fn build_stream(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        samples: Arc<Mutex<Vec<f32>>>,
    ) -> Result<Stream, String> {
        let stream_config: StreamConfig = config.clone().into();
        let on_error = |error| eprintln!("Audio input stream error: {error}");
        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    samples
                        .lock()
                        .expect("recording buffer lock")
                        .extend_from_slice(data);
                },
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    samples
                        .lock()
                        .expect("recording buffer lock")
                        .extend(data.iter().map(|sample| *sample as f32 / i16::MAX as f32));
                },
                on_error,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    samples.lock().expect("recording buffer lock").extend(
                        data.iter()
                            .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0),
                    );
                },
                on_error,
                None,
            ),
            sample_format => return Err(format!("Unsupported microphone format: {sample_format}")),
        };
        stream.map_err(|error| format!("Unable to open microphone: {error}"))
    }

    fn preferred_input_config(
        device: &cpal::Device,
    ) -> Result<cpal::SupportedStreamConfig, String> {
        let default = device
            .default_input_config()
            .map_err(|error| format!("Unable to read default microphone settings: {error}"))?;
        let ranges = device
            .supported_input_configs()
            .map_err(|error| format!("Unable to read microphone capabilities: {error}"))?;
        let mut highest_compatible: Option<cpal::SupportedStreamConfig> = None;
        for range in ranges {
            if range.sample_format() != default.sample_format()
                || range.channels() != default.channels()
            {
                continue;
            }
            let min = range.min_sample_rate().0;
            let max = range.max_sample_rate().0;
            if min <= 48_000 && max >= 48_000 {
                return Ok(range.with_sample_rate(cpal::SampleRate(48_000)));
            }
            if highest_compatible
                .as_ref()
                .is_none_or(|current| max > current.sample_rate().0)
            {
                highest_compatible = Some(range.with_sample_rate(cpal::SampleRate(max)));
            }
        }
        Ok(highest_compatible.unwrap_or(default))
    }

    fn write_wav(
        &self,
        samples: &[f32],
        config: &StreamConfig,
        path: &Path,
    ) -> Result<f64, String> {
        let spec = hound::WavSpec {
            channels: config.channels,
            sample_rate: config.sample_rate.0,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)
            .map_err(|error| format!("Unable to create WAV recording: {error}"))?;
        for sample in samples {
            writer
                .write_sample(sample.clamp(-1.0, 1.0))
                .map_err(|error| format!("Unable to write recorded audio: {error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("Unable to finalize WAV recording: {error}"))?;
        Ok(samples.len() as f64 / config.channels as f64 / config.sample_rate.0 as f64)
    }

    fn unique_path(&self, suffix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.output_dir.join(format!("recording-{stamp}{suffix}"))
    }

    fn media_duration(&self, source: &str) -> Result<f64, String> {
        let context = ffmpeg_next::format::input(source)
            .map_err(|error| format!("Unable to inspect recorded media: {error}"))?;
        let duration = context.duration();
        if duration <= 0 {
            return Err("The recording has no readable duration.".into());
        }
        Ok(duration as f64 / ffmpeg_next::ffi::AV_TIME_BASE as f64)
    }

    fn media_sample_rate(&self, source: &str) -> Result<u32, String> {
        let input = ffmpeg_next::format::input(source)
            .map_err(|error| format!("Unable to inspect recorded media: {error}"))?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .ok_or("The selected file does not contain an audio stream.")?;
        let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|error| format!("Unable to read audio stream settings: {error}"))?;
        let decoder = context
            .decoder()
            .audio()
            .map_err(|error| format!("Unable to open audio stream settings: {error}"))?;
        let sample_rate = decoder.rate();
        if sample_rate == 0 {
            return Err("The audio stream has no sample rate.".into());
        }
        Ok(sample_rate)
    }

    fn run_ffmpeg(&self, args: &[String]) -> Result<String, String> {
        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .map_err(|error| format!("Unable to run FFmpeg: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stderr).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn run_ffmpeg_command(args: &[String]) -> Result<String, String> {
        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .map_err(|error| format!("Unable to run FFmpeg: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stderr).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn measure_lufs(&self, source: &str) -> Result<f64, String> {
        let report = self.run_ffmpeg(&[
            "-hide_banner".into(),
            "-i".into(),
            source.into(),
            "-filter_complex".into(),
            "ebur128=peak=true".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ])?;
        let marker = "I:";
        let value = report
            .rsplit(marker)
            .next()
            .and_then(|line| line.trim().split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok());
        value.ok_or_else(|| "FFmpeg could not measure integrated loudness.".into())
    }

    fn loudnorm_measurement(
        &self,
        source: &str,
        target_lufs: f64,
    ) -> Result<serde_json::Value, String> {
        let filter = format!("loudnorm=I={target_lufs}:TP=-1.5:LRA=11:print_format=json");
        let report = self.run_ffmpeg(&[
            "-hide_banner".into(),
            "-i".into(),
            source.into(),
            "-af".into(),
            filter,
            "-f".into(),
            "null".into(),
            "-".into(),
        ])?;
        let start = report
            .rfind("{\n")
            .ok_or("FFmpeg did not return loudness statistics.")?;
        let end = report[start..]
            .find("\n}")
            .ok_or("FFmpeg returned incomplete loudness statistics.")?
            + start
            + 2;
        serde_json::from_str(&report[start..end])
            .map_err(|error| format!("Unable to read FFmpeg loudness statistics: {error}"))
    }

    fn validate_target_lufs(target_lufs: f64) -> Result<(), String> {
        if target_lufs.is_finite() && (-70.0..=-5.0).contains(&target_lufs) {
            Ok(())
        } else {
            Err("Loudness target must be between -70 and -5 LUFS.".into())
        }
    }
}

impl AudioEngine for NativeAudioEngine {
    fn start_recording(&self) -> Result<(), String> {
        let mut active = self
            .recording
            .lock()
            .map_err(|_| "Recording state is unavailable.".to_string())?;
        if active.is_some() {
            return Err("A recording is already in progress.".into());
        }
        let samples = Arc::new(Mutex::new(Vec::new()));
        let stream_samples = samples.clone();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<StreamConfig, String>>();
        let worker = std::thread::spawn(move || -> Result<(), String> {
            let startup = (|| -> Result<(Stream, StreamConfig), String> {
                let device = cpal::default_host()
                    .default_input_device()
                    .ok_or("No default microphone was found.")?;
                let supported_config = Self::preferred_input_config(&device)?;
                let config: StreamConfig = supported_config.clone().into();
                let stream = Self::build_stream(&device, &supported_config, stream_samples)?;
                stream
                    .play()
                    .map_err(|error| format!("Unable to start microphone: {error}"))?;
                Ok((stream, config))
            })();
            let (stream, config) = match startup {
                Ok(startup) => startup,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.clone()));
                    return Err(error);
                }
            };
            ready_tx
                .send(Ok(config))
                .map_err(|_| "Recording startup was cancelled.".to_string())?;
            stop_rx
                .recv()
                .map_err(|_| "Recording control channel closed unexpectedly.".to_string())?;
            drop(stream);
            Ok(())
        });
        let config = ready_rx.recv().map_err(|_| {
            "Recording thread stopped before microphone initialization.".to_string()
        })??;
        *active = Some(ActiveRecording {
            stop_tx,
            worker,
            samples,
            config,
        });
        Ok(())
    }

    fn stop_recording(&self, target_lufs: f64) -> Result<RecordingInfo, String> {
        Self::validate_target_lufs(target_lufs)?;
        let active = self
            .recording
            .lock()
            .map_err(|_| "Recording state is unavailable.".to_string())?
            .take()
            .ok_or("No recording is in progress.")?;
        active
            .stop_tx
            .send(())
            .map_err(|_| "Recording control channel closed unexpectedly.".to_string())?;
        active
            .worker
            .join()
            .map_err(|_| "Recording thread panicked.".to_string())??;
        let samples = active
            .samples
            .lock()
            .map_err(|_| "Recording data is unavailable.".to_string())?
            .clone();
        if samples.is_empty() {
            return Err("The microphone did not produce any audio.".into());
        }
        let raw_path = self.unique_path("-raw.wav");
        let duration_seconds = self.write_wav(&samples, &active.config, &raw_path)?;
        let mut info = self.normalize_to_lufs(&raw_path.display().to_string(), target_lufs)?;
        info.duration_seconds = duration_seconds;
        let _ = fs::remove_file(raw_path);
        Ok(info)
    }

    fn import_audio(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String> {
        Self::validate_target_lufs(target_lufs)?;
        let source_path = Path::new(source);
        if !source_path.is_file() {
            return Err("The selected audio file does not exist.".into());
        }
        let duration_seconds = self.media_duration(source)?;
        let mut info = self.normalize_to_lufs(source, target_lufs)?;
        info.name = source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Imported audio")
            .to_string();
        info.duration_seconds = duration_seconds;
        Ok(info)
    }

    fn normalize_to_lufs(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String> {
        Self::validate_target_lufs(target_lufs)?;
        let output_path = self.unique_path(".wav");
        let sample_rate = self.media_sample_rate(source)?;
        let measurement = self.loudnorm_measurement(source, target_lufs)?;
        let stat = |name: &str| {
            measurement[name]
                .as_str()
                .ok_or_else(|| format!("FFmpeg omitted {name} from loudness statistics."))
        };
        let filter = format!(
            "loudnorm=I={target_lufs}:TP=-1.5:LRA=11:measured_I={}:measured_LRA={}:measured_TP={}:measured_thresh={}:offset={}:linear=false:print_format=summary",
            stat("input_i")?, stat("input_lra")?, stat("input_tp")?, stat("input_thresh")?, stat("target_offset")?
        );
        self.run_ffmpeg(&[
            "-y".into(),
            "-i".into(),
            source.into(),
            "-af".into(),
            filter,
            "-ar".into(),
            sample_rate.to_string(),
            "-c:a".into(),
            "pcm_s24le".into(),
            output_path.display().to_string(),
        ])?;
        let lufs = self.measure_lufs(&output_path.display().to_string()).ok();
        let name = output_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Recording")
            .to_string();
        Ok(RecordingInfo {
            id: output_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("recording")
                .to_string(),
            name,
            path: output_path.display().to_string(),
            duration_seconds: 0.0,
            integrated_lufs: lufs,
        })
    }

    fn start_denoise(
        &self,
        recording_id: String,
        source: String,
        completion: DenoiseCompletion,
    ) -> Result<(), String> {
        let output_dir = self.output_dir.clone();
        let (python, worker) = self.clearvoice_runtime()?;

        std::thread::spawn(move || {
            let result = (|| -> Result<DenoiseResult, String> {
                let input_rate = Self::media_sample_rate_for(&source)?;
                let (model_name, model_rate) = if input_rate >= 44_100 {
                    ("MossFormer2_SE_48K", 48_000)
                } else {
                    ("FRCRN_SE_16K", 16_000)
                };
                let input = output_dir.join(format!("denoise-{}-input.wav", recording_id));
                let model_output = output_dir.join(format!("denoise-{}-model.wav", recording_id));
                let processed = output_dir.join(format!("denoise-{}-processed.wav", recording_id));
                Self::run_ffmpeg_command(&[
                    "-y".into(),
                    "-i".into(),
                    source.clone(),
                    "-ar".into(),
                    model_rate.to_string(),
                    "-c:a".into(),
                    "pcm_s16le".into(),
                    input.display().to_string(),
                ])?;
                let process = Command::new(python)
                    .current_dir(&output_dir)
                    .arg(worker)
                    .arg("--model")
                    .arg(model_name)
                    .arg("--input")
                    .arg(&input)
                    .arg("--output")
                    .arg(&model_output)
                    .output()
                    .map_err(|error| format!("Unable to start ClearVoice: {error}"))?;
                if !process.status.success() {
                    return Err(format!(
                        "ClearVoice failed: {}",
                        String::from_utf8_lossy(&process.stderr)
                    ));
                }
                Self::run_ffmpeg_command(&[
                    "-y".into(),
                    "-i".into(),
                    model_output.display().to_string(),
                    "-af".into(),
                    "adeclick=window=55:overlap=75:arorder=2:threshold=2:burst=2".into(),
                    "-ar".into(),
                    input_rate.to_string(),
                    "-c:a".into(),
                    "pcm_s24le".into(),
                    processed.display().to_string(),
                ])?;
                let _ = fs::remove_file(input);
                let _ = fs::remove_file(model_output);
                let analyzer = NativeAudioEngine {
                    recording: Mutex::new(None),
                    output_dir: output_dir.clone(),
                };
                Ok(DenoiseResult {
                    recording_id,
                    path: processed.display().to_string(),
                    integrated_lufs: analyzer.measure_lufs(&processed.display().to_string()).ok(),
                })
            })();
            completion(result);
        });
        Ok(())
    }

    fn export_edit(
        &self,
        source: &str,
        deleted_regions: &[Region],
        envelope_points: &[EnvelopePoint],
    ) -> Result<String, String> {
        let mut deletions = deleted_regions.to_vec();
        deletions.sort_by(|left, right| left.start.total_cmp(&right.start));
        let duration = self.media_duration(source)?;
        let mut cursor = 0.0;
        let mut filters = Vec::new();
        let mut labels = Vec::new();
        for (index, deletion) in deletions.iter().enumerate() {
            let end = deletion.start.clamp(cursor, duration);
            if end > cursor {
                let label = format!("a{index}");
                filters.push(format!(
                    "[input]atrim=start={cursor}:end={end},asetpts=PTS-STARTPTS[{label}]"
                ));
                labels.push(format!("[{label}]"));
            }
            cursor = deletion.end.clamp(cursor, duration);
        }
        if cursor < duration {
            let label = format!("a{}", labels.len());
            filters.push(format!(
                "[input]atrim=start={cursor}:end={duration},asetpts=PTS-STARTPTS[{label}]"
            ));
            labels.push(format!("[{label}]"));
        }
        if labels.is_empty() {
            return Err("The selected edits remove the whole recording.".into());
        }
        let mut points = envelope_points.to_vec();
        points.sort_by(|left, right| left.time.total_cmp(&right.time));
        points.insert(
            0,
            EnvelopePoint {
                time: 0.0,
                gain: 1.0,
            },
        );
        points.push(EnvelopePoint {
            time: duration,
            gain: 1.0,
        });
        let envelope_filter = if envelope_points.is_empty() {
            None
        } else {
            let segments = points
                .windows(2)
                .filter_map(|segment| {
                    let left = &segment[0];
                    let right = &segment[1];
                    let segment_duration = right.time - left.time;
                    if segment_duration <= 0.000_001 {
                        return None;
                    }
                    let progress = format!("(t-{})/{segment_duration}", left.time);
                    let smooth = format!("{progress}*{progress}*(3-2*{progress})");
                    let gain = format!(
                        "{}+({}-{})*{smooth}",
                        left.gain.clamp(0.0, 2.0),
                        right.gain.clamp(0.0, 2.0),
                        left.gain.clamp(0.0, 2.0)
                    );
                    Some(format!(
                        "if(between(t\\,{}\\,{})\\,{}\\,",
                        left.time, right.time, gain
                    ))
                })
                .collect::<Vec<_>>();
            let expression = format!("{}1{}", segments.join(""), ")".repeat(segments.len()));
            Some(format!("volume='{expression}':eval=frame"))
        };
        let source_filter = if let Some(envelope_filter) = envelope_filter {
            format!("[0:a]{envelope_filter},anull[input]")
        } else {
            "[0:a]anull[input]".to_string()
        };
        filters.insert(0, source_filter);
        filters.push(format!(
            "{}concat=n={}:v=0:a=1[out]",
            labels.join(""),
            labels.len()
        ));
        let output = self.unique_path("-edited.wav");
        self.run_ffmpeg(&[
            "-y".into(),
            "-i".into(),
            source.into(),
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[out]".into(),
            "-c:a".into(),
            "pcm_s24le".into(),
            output.display().to_string(),
        ])?;
        Ok(output.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::NativeAudioEngine;

    #[test]
    fn validates_ffmpeg_loudness_target_range() {
        assert!(NativeAudioEngine::validate_target_lufs(-70.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-15.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-5.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-70.1).is_err());
        assert!(NativeAudioEngine::validate_target_lufs(-4.9).is_err());
        assert!(NativeAudioEngine::validate_target_lufs(f64::NAN).is_err());
    }
}

impl NativeAudioEngine {
    fn media_sample_rate_for(source: &str) -> Result<u32, String> {
        let input = ffmpeg_next::format::input(source)
            .map_err(|error| format!("Unable to inspect recorded media: {error}"))?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .ok_or("The selected file does not contain an audio stream.")?;
        let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|error| format!("Unable to read audio stream settings: {error}"))?;
        let decoder = context
            .decoder()
            .audio()
            .map_err(|error| format!("Unable to open audio stream settings: {error}"))?;
        let sample_rate = decoder.rate();
        if sample_rate == 0 {
            return Err("The audio stream has no sample rate.".into());
        }
        Ok(sample_rate)
    }
}
