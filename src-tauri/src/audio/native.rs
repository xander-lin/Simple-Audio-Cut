use super::{
    reserve_export_path, AudioEngine, DenoiseAvailability, DenoiseCompletion, DenoiseResult,
    DenoiseUpdate, EnvelopePoint, ExportEdit, ExportResult, RecordingInfo, Region,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    clearvoice_import: Mutex<Option<(PathBuf, bool)>>,
}

impl NativeAudioEngine {
    pub fn new(output_dir: PathBuf) -> Result<Self, String> {
        ffmpeg_next::init().map_err(|error| format!("Unable to initialize FFmpeg: {error}"))?;
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("Unable to create recording directory: {error}"))?;
        Ok(Self {
            recording: Mutex::new(None),
            output_dir,
            clearvoice_import: Mutex::new(None),
        })
    }

    fn clearvoice_runtime(&self, model_name: &str) -> Result<(PathBuf, PathBuf, PathBuf), String> {
        let python = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON")
            .map(PathBuf::from)
            .filter(|path| path.is_file() && self.python_has_clearvoice(path));

        let mut worker_candidates = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER")
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        #[cfg(debug_assertions)]
        if let Ok(current_dir) = std::env::current_dir() {
            worker_candidates.push(current_dir.join("tools/clearvoice_denoise.py"));
            if let Some(parent) = current_dir.parent() {
                worker_candidates.push(parent.join("tools/clearvoice_denoise.py"));
            }
        }
        worker_candidates.push(PathBuf::from(
            "/usr/lib/simple-audio-cut/clearvoice_denoise.py",
        ));
        let worker = worker_candidates.into_iter().find(|path| path.is_file());

        let model_root = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT")
            .map(PathBuf::from)
            .filter(|path| Self::has_clearvoice_model(path, model_name));

        match (python, worker, model_root) {
            (Some(python), Some(worker), Some(model_root)) => Ok((python, worker, model_root)),
            _ => Err("ClearVoice is not configured for this audio format.".into()),
        }
    }

    fn has_clearvoice_model(root: &Path, model_name: &str) -> bool {
        let model_dir = root.join("checkpoints").join(model_name);
        let marker = model_dir.join("last_best_checkpoint");
        let Ok(checkpoint_name) = fs::read_to_string(marker) else {
            return false;
        };
        let checkpoint = model_dir.join(checkpoint_name.trim());
        checkpoint
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
    }

    fn python_has_clearvoice(&self, python: &Path) -> bool {
        let mut cached = match self.clearvoice_import.lock() {
            Ok(cached) => cached,
            Err(_) => return false,
        };
        if let Some((cached_python, available)) = cached.as_ref() {
            if cached_python == python {
                return *available;
            }
        }
        let available = Command::new(python)
            .args(["-c", "import clearvoice"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        *cached = Some((python.to_path_buf(), available));
        available
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

    fn run_command(
        program: &Path,
        args: &[String],
        current_dir: Option<&Path>,
        label: &str,
    ) -> Result<String, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(current_dir) = current_dir {
            command.current_dir(current_dir);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Unable to start {label}: {error}"))?;
        let mut stdout = child.stdout.take().expect("piped child stdout");
        let mut stderr = child.stderr.take().expect("piped child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = stdout.read_to_end(&mut output);
            output
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = stderr.read_to_end(&mut output);
            output
        });
        let status = child
            .wait()
            .map_err(|error| format!("Unable to wait for {label}: {error}"))?;
        let _ = stdout_reader.join();
        let stderr = stderr_reader.join().unwrap_or_default();
        if status.success() {
            Ok(String::from_utf8_lossy(&stderr).into_owned())
        } else {
            Err(format!(
                "{label} failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            ))
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
        Self::parse_lufs_report(&report)
    }

    fn parse_lufs_report(report: &str) -> Result<f64, String> {
        let marker = "I:";
        let value = report
            .rsplit(marker)
            .next()
            .and_then(|line| line.split_whitespace().next())
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

    fn denoise_availability(&self, sample_rate: u32) -> DenoiseAvailability {
        let model_name = if sample_rate >= 44_100 {
            "MossFormer2_SE_48K"
        } else {
            "FRCRN_SE_16K"
        };
        DenoiseAvailability {
            available: sample_rate > 0 && self.clearvoice_runtime(model_name).is_ok(),
        }
    }

    fn start_denoise(
        &self,
        recording_id: String,
        task_id: String,
        source: String,
        sample_rate: u32,
        completion: DenoiseCompletion,
    ) -> Result<(), String> {
        let output_dir = self.output_dir.clone();
        if sample_rate == 0 {
            return Err("The audio stream has no sample rate.".into());
        }
        let (model_name, model_rate) = if sample_rate >= 44_100 {
            ("MossFormer2_SE_48K", 48_000)
        } else {
            ("FRCRN_SE_16K", 16_000)
        };
        let (python, worker, model_root) = self.clearvoice_runtime(model_name)?;

        std::thread::spawn(move || {
            let run_denoise = || -> Result<DenoiseResult, String> {
                let input = output_dir.join(format!("denoise-{task_id}-input.wav"));
                let model_output = output_dir.join(format!("denoise-{task_id}-model.wav"));
                let processed = output_dir.join(format!("denoise-{task_id}-processed.wav"));
                let _ = fs::remove_file(&input);
                let _ = fs::remove_file(&model_output);
                let _ = fs::remove_file(&processed);
                let outcome = (|| {
                    Self::run_command(
                        Path::new("ffmpeg"),
                        &[
                            "-y".into(),
                            "-i".into(),
                            source.clone(),
                            "-ar".into(),
                            model_rate.to_string(),
                            "-c:a".into(),
                            "pcm_s16le".into(),
                            input.display().to_string(),
                        ],
                        None,
                        "FFmpeg denoise preparation",
                    )?;
                    Self::run_command(
                        &python,
                        &[
                            worker.display().to_string(),
                            "--model".into(),
                            model_name.into(),
                            "--input".into(),
                            input.display().to_string(),
                            "--output".into(),
                            model_output.display().to_string(),
                        ],
                        Some(&model_root),
                        "ClearVoice",
                    )?;
                    Self::run_command(
                        Path::new("ffmpeg"),
                        &[
                            "-y".into(),
                            "-i".into(),
                            model_output.display().to_string(),
                            "-af".into(),
                            "adeclick=window=55:overlap=75:arorder=2:threshold=2:burst=2".into(),
                            "-ar".into(),
                            sample_rate.to_string(),
                            "-c:a".into(),
                            "pcm_s24le".into(),
                            processed.display().to_string(),
                        ],
                        None,
                        "FFmpeg denoise post-processing",
                    )?;
                    Ok::<(), String>(())
                })();
                let _ = fs::remove_file(input);
                let _ = fs::remove_file(model_output);
                if outcome.is_err() {
                    let _ = fs::remove_file(&processed);
                }
                outcome?;
                let integrated_lufs = Self::run_command(
                    Path::new("ffmpeg"),
                    &[
                        "-hide_banner".into(),
                        "-i".into(),
                        processed.display().to_string(),
                        "-filter_complex".into(),
                        "ebur128=peak=true".into(),
                        "-f".into(),
                        "null".into(),
                        "-".into(),
                    ],
                    None,
                    "FFmpeg loudness measurement",
                )
                .ok()
                .and_then(|report| Self::parse_lufs_report(&report).ok());
                Ok(DenoiseResult {
                    recording_id: recording_id.clone(),
                    task_id: task_id.clone(),
                    path: processed.display().to_string(),
                    integrated_lufs,
                })
            };
            completion(DenoiseUpdate::Processing {
                recording_id: recording_id.clone(),
                task_id: task_id.clone(),
            });
            match run_denoise() {
                Ok(result) => completion(DenoiseUpdate::Complete { result }),
                Err(error) => completion(DenoiseUpdate::Failed {
                    recording_id,
                    task_id,
                    error,
                }),
            }
        });
        Ok(())
    }

    fn export_edit(
        &self,
        source: &str,
        deleted_regions: &[Region],
        envelope_points: &[EnvelopePoint],
        destination: &str,
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
        let mut output = PathBuf::from(destination);
        if output.extension().is_none() {
            output.set_extension("wav");
        }
        let parent = output
            .parent()
            .ok_or("The selected export location has no parent directory.")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create the export directory: {error}"))?;
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
        let metadata = fs::metadata(&output)
            .map_err(|error| format!("FFmpeg did not create the export file: {error}"))?;
        if metadata.len() == 0 {
            return Err("FFmpeg created an empty export file.".into());
        }
        Ok(output.display().to_string())
    }

    fn export_edits(&self, edits: &[ExportEdit], destination_dir: &str) -> Vec<ExportResult> {
        let destination_dir = Path::new(destination_dir);
        if let Err(error) = fs::create_dir_all(destination_dir) {
            return edits
                .iter()
                .map(|edit| ExportResult {
                    recording_id: edit.recording_id.clone(),
                    path: None,
                    error: Some(format!("Unable to create the export directory: {error}")),
                })
                .collect();
        }
        edits
            .iter()
            .map(|edit| {
                let destination = match reserve_export_path(destination_dir, &edit.name) {
                    Ok(destination) => destination,
                    Err(error) => {
                        return ExportResult {
                            recording_id: edit.recording_id.clone(),
                            path: None,
                            error: Some(error),
                        };
                    }
                };
                let result = self.export_edit(
                    &edit.source_path,
                    &edit.deleted_regions,
                    &edit.envelope_points,
                    &destination.display().to_string(),
                );
                match result {
                    Ok(path) => ExportResult {
                        recording_id: edit.recording_id.clone(),
                        path: Some(path),
                        error: None,
                    },
                    Err(error) => {
                        let _ = fs::remove_file(destination);
                        ExportResult {
                            recording_id: edit.recording_id.clone(),
                            path: None,
                            error: Some(error),
                        }
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{reserve_export_path, AudioEngine, DenoiseUpdate, ExportEdit, NativeAudioEngine};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn validates_ffmpeg_loudness_target_range() {
        assert!(NativeAudioEngine::validate_target_lufs(-70.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-15.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-5.0).is_ok());
        assert!(NativeAudioEngine::validate_target_lufs(-70.1).is_err());
        assert!(NativeAudioEngine::validate_target_lufs(-4.9).is_err());
        assert!(NativeAudioEngine::validate_target_lufs(f64::NAN).is_err());
    }

    #[test]
    fn exports_directly_to_the_requested_destination() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-export-{stamp}"));
        let source = root.join("source.wav");
        let destination = root.join("chosen-location/export.wav");
        fs::create_dir_all(&root).expect("create test directory");

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).expect("create source WAV");
        for _ in 0..4_800 {
            writer
                .write_sample::<i16>(1_000)
                .expect("write source sample");
        }
        writer.finalize().expect("finalize source WAV");

        let engine = NativeAudioEngine::new(root.join("recordings")).expect("create audio engine");
        let exported = engine
            .export_edit(
                &source.display().to_string(),
                &[],
                &[],
                &destination.display().to_string(),
            )
            .expect("export to requested path");

        assert_eq!(exported, destination.display().to_string());
        assert!(fs::metadata(&destination).expect("inspect export").len() > 44);
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn creates_non_overwriting_batch_export_paths() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-batch-path-{stamp}"));
        fs::create_dir_all(&root).expect("create test directory");
        fs::write(root.join("voice-edited.wav"), b"existing").expect("create existing export");

        let path = reserve_export_path(&root, "voice").expect("reserve voice path");
        let sanitized = reserve_export_path(&root, "folder/voice").expect("reserve sanitized path");

        assert_eq!(path, root.join("voice-edited-2.wav"));
        assert_eq!(sanitized, root.join("folder_voice-edited.wav"));
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn rejects_an_incomplete_clearvoice_model_directory() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-model-path-{stamp}"));
        let incomplete = root.join("incomplete");
        let complete = root.join("complete");
        let model_name = "MossFormer2_SE_48K";
        for candidate in [&incomplete, &complete] {
            let model_dir = candidate.join("checkpoints").join(model_name);
            fs::create_dir_all(&model_dir).expect("create model directory");
            fs::write(model_dir.join("last_best_checkpoint"), "model.pt\n")
                .expect("write model marker");
        }
        fs::write(
            complete
                .join("checkpoints")
                .join(model_name)
                .join("model.pt"),
            b"model",
        )
        .expect("write complete model");

        assert!(!NativeAudioEngine::has_clearvoice_model(
            &incomplete,
            model_name
        ));
        assert!(NativeAudioEngine::has_clearvoice_model(
            &complete, model_name
        ));
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn rejects_a_python_without_clearvoice() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-python-path-{stamp}"));
        fs::create_dir_all(&root).expect("create test directory");
        let engine = NativeAudioEngine::new(root.join("recordings")).expect("create audio engine");

        assert!(!engine.python_has_clearvoice(Path::new("/bin/false")));
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn exports_multiple_edits_in_one_batch() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-batch-{stamp}"));
        let source = root.join("source.wav");
        let destination = root.join("exports");
        fs::create_dir_all(&root).expect("create test directory");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).expect("create source WAV");
        for _ in 0..4_800 {
            writer
                .write_sample::<i16>(1_000)
                .expect("write source sample");
        }
        writer.finalize().expect("finalize source WAV");
        let engine = NativeAudioEngine::new(root.join("recordings")).expect("create audio engine");
        let edits = ["first", "second"].map(|name| ExportEdit {
            recording_id: name.into(),
            name: name.into(),
            source_path: source.display().to_string(),
            deleted_regions: Vec::new(),
            envelope_points: Vec::new(),
        });

        let results = engine.export_edits(&edits, &destination.display().to_string());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.error.is_none()));
        assert!(destination.join("first-edited.wav").is_file());
        assert!(destination.join("second-edited.wav").is_file());
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    #[ignore = "runs the installed ClearVoice model"]
    fn denoises_with_the_real_clearvoice_runtime() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-clearvoice-{stamp}"));
        let source = root.join("source.wav");
        fs::create_dir_all(&root).expect("create test directory");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).expect("create source WAV");
        for sample in 0..480_000 {
            let frame = sample / 2;
            let phase = frame as f32 * 440.0 * std::f32::consts::TAU / 48_000.0;
            writer
                .write_sample::<i16>((phase.sin() * 4_000.0) as i16)
                .expect("write source sample");
        }
        writer.finalize().expect("finalize source WAV");
        let engine = NativeAudioEngine::new(root.join("recordings")).expect("create audio engine");
        let (tx, rx) = std::sync::mpsc::channel();

        engine
            .start_denoise(
                "recording".into(),
                "real-model".into(),
                source.display().to_string(),
                48_000,
                Box::new(move |update| tx.send(update).expect("send denoise update")),
            )
            .expect("start denoise");

        let result = loop {
            match rx.recv().expect("receive denoise update") {
                DenoiseUpdate::Processing { .. } => {}
                DenoiseUpdate::Complete { result } => break Ok(result),
                DenoiseUpdate::Failed { error, .. } => break Err(error),
            }
        }
        .expect("real ClearVoice denoise should complete");

        assert!(Path::new(&result.path).is_file());
        assert!(fs::metadata(&result.path).expect("inspect output").len() > 44);
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
