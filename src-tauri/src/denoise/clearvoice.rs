use super::{
    DenoiseCheck, DenoiseCheckState, DenoiseConfig, DenoiseDeviceInfo, DenoiseModelInfo,
    DenoiseProvider, DenoiseProviderStatus, DenoiseSession,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

const HIGH_RATE_MODEL: &str = "MossFormer2_SE_48K";
const LOW_RATE_MODEL: &str = "FRCRN_SE_16K";
const LEGACY_APP_IDENTIFIER: &str = "io.github.xander_lin.simple_audio_cut";
const WORKER_PROTOCOL_PREFIX: &str = "SIMPLE_AUDIO_CUT_WORKER=";

pub struct ClearVoiceProvider {
    config_path: PathBuf,
    config: RwLock<DenoiseConfig>,
    config_error: RwLock<Option<String>>,
    clearvoice_import: Mutex<Option<(PathBuf, bool)>>,
    clearvoice_device_probe: Mutex<Option<(PathBuf, String, bool)>>,
    cuda_devices: Mutex<Option<(PathBuf, Vec<DenoiseDeviceInfo>)>>,
    worker_pool: Arc<ClearVoiceWorkerPool>,
}

impl ClearVoiceProvider {
    pub fn new(config_path: PathBuf) -> Self {
        let (config, mut config_error) = match fs::read_to_string(&config_path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(config) => (config, None),
                Err(error) => (
                    DenoiseConfig::default(),
                    Some(format!("Unable to read saved denoising settings: {error}")),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (DenoiseConfig::default(), None)
            }
            Err(error) => (
                DenoiseConfig::default(),
                Some(format!("Unable to read saved denoising settings: {error}")),
            ),
        };
        let configured = Self::normalize_config(config);
        let (configured, migrated) = Self::migrate_legacy_config(
            configured,
            std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON").map(PathBuf::from),
            std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT").map(PathBuf::from),
        );
        if migrated {
            if let Err(error) = Self::write_config_file(&config_path, &configured) {
                config_error = Some(error);
            }
        }
        Self {
            config_path,
            config: RwLock::new(configured),
            config_error: RwLock::new(config_error),
            clearvoice_import: Mutex::new(None),
            clearvoice_device_probe: Mutex::new(None),
            cuda_devices: Mutex::new(None),
            worker_pool: Arc::new(ClearVoiceWorkerPool::new()),
        }
    }

    fn normalize_config(mut config: DenoiseConfig) -> DenoiseConfig {
        config.executable_path = config.executable_path.trim().to_string();
        config.model_root = Self::normalize_model_root(config.model_root.trim())
            .display()
            .to_string();
        config.device = Self::normalize_device(&config.device).unwrap_or_else(|_| "auto".into());
        config.high_sample_rate_model = config.high_sample_rate_model.trim().to_string();
        config.low_sample_rate_model = config.low_sample_rate_model.trim().to_string();
        if config.high_sample_rate_model.is_empty() {
            config.high_sample_rate_model = HIGH_RATE_MODEL.into();
        }
        if config.low_sample_rate_model.is_empty() {
            config.low_sample_rate_model = LOW_RATE_MODEL.into();
        }
        config
    }

    fn normalize_model_root(value: &str) -> PathBuf {
        if value.is_empty() {
            return PathBuf::new();
        }
        let selected = PathBuf::from(value);
        if selected.file_name().and_then(|name| name.to_str()) == Some("checkpoints") {
            return selected.parent().unwrap_or(&selected).to_path_buf();
        }
        if selected.join("last_best_checkpoint").is_file()
            && selected
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some("checkpoints")
        {
            return selected
                .parent()
                .and_then(Path::parent)
                .unwrap_or(&selected)
                .to_path_buf();
        }
        selected
    }

    fn legacy_clearvoice_root(path: &Path) -> Option<PathBuf> {
        path.ancestors().find_map(|ancestor| {
            (ancestor.file_name().and_then(|name| name.to_str()) == Some(LEGACY_APP_IDENTIFIER))
                .then(|| ancestor.join("clearvoice"))
                .filter(|root| root.join("checkpoints").is_dir())
        })
    }

    fn is_legacy_managed_path(path: &Path) -> bool {
        path.ancestors().any(|ancestor| {
            ancestor.file_name().and_then(|name| name.to_str()) == Some(LEGACY_APP_IDENTIFIER)
        })
    }

    fn migrate_legacy_config(
        mut config: DenoiseConfig,
        environment_python: Option<PathBuf>,
        environment_model_root: Option<PathBuf>,
    ) -> (DenoiseConfig, bool) {
        let original = config.clone();
        let configured_python = PathBuf::from(&config.executable_path);
        let configured_model_root = PathBuf::from(&config.model_root);
        let clearvoice_root = [
            environment_model_root.as_deref(),
            environment_python.as_deref(),
            (!config.model_root.is_empty()).then_some(configured_model_root.as_path()),
            (!config.executable_path.is_empty()).then_some(configured_python.as_path()),
        ]
        .into_iter()
        .flatten()
        .find_map(Self::legacy_clearvoice_root);

        if config.executable_path.is_empty() {
            let executable = environment_python
                .filter(|path| Self::is_legacy_managed_path(path) && path.is_file())
                .or_else(|| {
                    clearvoice_root
                        .as_ref()
                        .map(|root| root.join(".venv/bin/python"))
                        .filter(|path| path.is_file())
                });
            if let Some(executable) = executable {
                config.executable_path = executable.display().to_string();
            }
        }

        let configured_root_is_legacy_recordings = configured_model_root
            .file_name()
            .and_then(|name| name.to_str())
            == Some("recordings")
            && Self::is_legacy_managed_path(&configured_model_root);
        if config.model_root.is_empty() || configured_root_is_legacy_recordings {
            if let Some(root) = clearvoice_root {
                config.model_root = root.display().to_string();
            }
        }

        let config = Self::normalize_config(config);
        let migrated = config != original;
        (config, migrated)
    }

    fn configured(&self) -> DenoiseConfig {
        self.config
            .read()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    fn effective_config(configured: &DenoiseConfig) -> (DenoiseConfig, Vec<String>) {
        let mut effective = configured.clone();
        let mut overrides = Vec::new();
        if let Some(value) = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON") {
            let path = PathBuf::from(value);
            if !Self::is_legacy_managed_path(&path) {
                effective.executable_path = path.display().to_string();
                overrides.push("Python executable".into());
            }
        }
        if let Some(value) = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT") {
            let path = PathBuf::from(value);
            if !Self::is_legacy_managed_path(&path) {
                effective.model_root = Self::normalize_model_root(&path.display().to_string())
                    .display()
                    .to_string();
                overrides.push("Model library".into());
            }
        }
        if let Ok(value) = std::env::var("SIMPLE_AUDIO_CUT_CLEARVOICE_DEVICE") {
            effective.device = value;
            overrides.push("Processing device".into());
        }
        if std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER").is_some() {
            overrides.push("ClearVoice adapter".into());
        }
        (Self::normalize_config(effective), overrides)
    }

    fn worker_path() -> Option<PathBuf> {
        if let Some(worker) = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER") {
            return Some(PathBuf::from(worker));
        }
        let mut candidates = Vec::new();
        #[cfg(debug_assertions)]
        if let Ok(current_dir) = std::env::current_dir() {
            candidates.push(current_dir.join("tools/clearvoice_denoise.py"));
            if let Some(parent) = current_dir.parent() {
                candidates.push(parent.join("tools/clearvoice_denoise.py"));
            }
        }
        candidates.push(PathBuf::from(
            "/usr/lib/simple-audio-cut/clearvoice_denoise.py",
        ));
        candidates.into_iter().find(|path| path.is_file())
    }

    fn clearvoice_model_error(root: &Path, model_name: &str) -> Option<String> {
        let model_dir = root.join("checkpoints").join(model_name);
        let marker = model_dir.join("last_best_checkpoint");
        let checkpoint_name = match fs::read_to_string(&marker) {
            Ok(checkpoint_name) => checkpoint_name,
            Err(_) => {
                return Some(format!("Missing checkpoint marker: {}.", marker.display()));
            }
        };
        let checkpoint_name = checkpoint_name.trim();
        if checkpoint_name.is_empty() {
            return Some(format!("Checkpoint marker is empty: {}.", marker.display()));
        }
        let checkpoint = model_dir.join(checkpoint_name);
        match checkpoint.metadata() {
            Ok(metadata) if metadata.is_file() && metadata.len() > 0 => None,
            Ok(_) => Some(format!(
                "Checkpoint file is empty or invalid: {}.",
                checkpoint.display()
            )),
            Err(_) => Some(format!(
                "Missing checkpoint file: {}.",
                checkpoint.display()
            )),
        }
    }

    pub(crate) fn has_clearvoice_model(root: &Path, model_name: &str) -> bool {
        Self::clearvoice_model_error(root, model_name).is_none()
    }

    fn inferred_sample_rate(model_name: &str) -> Option<u32> {
        let upper = model_name.to_ascii_uppercase();
        if upper.contains("48K") {
            Some(48_000)
        } else if upper.contains("16K") {
            Some(16_000)
        } else {
            None
        }
    }

    fn scan_models(root: &Path) -> Vec<DenoiseModelInfo> {
        let Ok(entries) = fs::read_dir(root.join("checkpoints")) else {
            return Vec::new();
        };
        let mut models = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .map(|id| {
                let model_error = Self::clearvoice_model_error(root, &id);
                DenoiseModelInfo {
                    sample_rate: Self::inferred_sample_rate(&id),
                    detail: model_error
                        .clone()
                        .unwrap_or_else(|| "Checkpoint is ready".into()),
                    id,
                    ready: model_error.is_none(),
                }
            })
            .collect::<Vec<_>>();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models
    }

    fn clear_validation_caches(&self) {
        if let Ok(mut cache) = self.clearvoice_import.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.clearvoice_device_probe.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.cuda_devices.lock() {
            *cache = None;
        }
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

    fn normalize_device(raw: &str) -> Result<String, String> {
        let device = raw.trim().to_ascii_lowercase();
        if device.is_empty() || device == "auto" {
            return Ok("auto".into());
        }
        if device == "gpu" {
            return Ok("cuda".into());
        }
        if device == "cpu" || device == "cuda" {
            return Ok(device);
        }
        if let Some(index) = device.strip_prefix("cuda:") {
            if !index.is_empty() && index.chars().all(|character| character.is_ascii_digit()) {
                return Ok(device);
            }
        }
        Err("Processing device must be Automatic, CPU, or an available CUDA device.".into())
    }

    fn python_supports_device(&self, python: &Path, device: &str) -> bool {
        if device == "cpu" {
            return true;
        }
        let mut cached = match self.clearvoice_device_probe.lock() {
            Ok(cached) => cached,
            Err(_) => return false,
        };
        if let Some((cached_python, cached_device, available)) = cached.as_ref() {
            if cached_python == python && cached_device == device {
                return *available;
            }
        }
        let mut command = Command::new(python);
        command
            .args([
                "-c",
                "import torch; assert torch.cuda.is_available(); torch.cuda.set_device(0); assert (torch.ones((1,), device='cuda') + 1).cpu().item() == 2",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(index) = device.strip_prefix("cuda:") {
            command.env("CUDA_VISIBLE_DEVICES", index);
        }
        let available = command
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        *cached = Some((python.to_path_buf(), device.to_string(), available));
        available
    }

    fn device_options(&self, python: &Path) -> Vec<DenoiseDeviceInfo> {
        let mut options = vec![
            DenoiseDeviceInfo {
                id: "auto".into(),
                label: "Automatic".into(),
            },
            DenoiseDeviceInfo {
                id: "cpu".into(),
                label: "CPU".into(),
            },
        ];
        if !python.is_file() {
            return options;
        }
        let mut cached = match self.cuda_devices.lock() {
            Ok(cached) => cached,
            Err(_) => return options,
        };
        let devices = if let Some((cached_python, devices)) = cached.as_ref() {
            if cached_python == python {
                devices.clone()
            } else {
                Self::probe_cuda_devices(python)
            }
        } else {
            Self::probe_cuda_devices(python)
        };
        *cached = Some((python.to_path_buf(), devices.clone()));
        options.extend(devices);
        options
    }

    fn probe_cuda_devices(python: &Path) -> Vec<DenoiseDeviceInfo> {
        let output = Command::new(python)
            .args([
                "-c",
                "import json, torch; print('SIMPLE_AUDIO_CUT_DEVICES=' + json.dumps([torch.cuda.get_device_name(i) for i in range(torch.cuda.device_count())]))",
            ])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let devices = stdout
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("SIMPLE_AUDIO_CUT_DEVICES="))
            .unwrap_or("[]");
        serde_json::from_str::<Vec<String>>(devices)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, name)| DenoiseDeviceInfo {
                id: format!("cuda:{index}"),
                label: format!("GPU {index}: {name}"),
            })
            .collect()
    }

    fn resolved_device(&self, python: &Path, configured: &str) -> Result<String, String> {
        let device = Self::normalize_device(configured)?;
        if device == "auto" {
            return Ok(if self.python_supports_device(python, "cuda") {
                "cuda".into()
            } else {
                "cpu".into()
            });
        }
        if device != "cpu" && !self.python_supports_device(python, &device) {
            return Err(
                "The selected CUDA device is not available in this Python environment.".into(),
            );
        }
        Ok(device)
    }

    fn build_status(&self, configured: DenoiseConfig) -> DenoiseProviderStatus {
        let configured = Self::normalize_config(configured);
        let (effective, environment_overrides) = Self::effective_config(&configured);
        let python = PathBuf::from(&effective.executable_path);
        let model_root = PathBuf::from(&effective.model_root);
        let worker = Self::worker_path();
        let models = Self::scan_models(&model_root);
        let devices = self.device_options(&python);
        let mut checks = Vec::new();

        let python_exists = !effective.executable_path.is_empty() && python.is_file();
        checks.push(DenoiseCheck {
            id: "python".into(),
            label: "Python environment".into(),
            state: if python_exists {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: if effective.executable_path.is_empty() {
                "Choose the Python executable from your ClearVoice environment.".into()
            } else if python_exists {
                effective.executable_path.clone()
            } else {
                "The selected executable does not exist.".into()
            },
        });

        let import_ready = python_exists && self.python_has_clearvoice(&python);
        checks.push(DenoiseCheck {
            id: "clearvoice".into(),
            label: "ClearVoice package".into(),
            state: if import_ready {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: if import_ready {
                "ClearVoice imports successfully.".into()
            } else {
                "The selected Python environment cannot import clearvoice.".into()
            },
        });

        let worker_ready = worker.as_ref().is_some_and(|path| path.is_file());
        checks.push(DenoiseCheck {
            id: "adapter".into(),
            label: "Provider adapter".into(),
            state: if worker_ready {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: worker
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| {
                    "The ClearVoice adapter was not found in this installation.".into()
                }),
        });

        let model_root_ready =
            !effective.model_root.is_empty() && model_root.join("checkpoints").is_dir();
        checks.push(DenoiseCheck {
            id: "library".into(),
            label: "Model library".into(),
            state: if model_root_ready {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: if effective.model_root.is_empty() {
                "Choose the folder that contains checkpoints/.".into()
            } else if model_root_ready {
                format!("{} model folders found.", models.len())
            } else {
                "The selected folder does not contain checkpoints/.".into()
            },
        });

        let high_model_error =
            Self::clearvoice_model_error(&model_root, &effective.high_sample_rate_model);
        let high_model_ready = high_model_error.is_none();
        checks.push(DenoiseCheck {
            id: "high-rate-model".into(),
            label: "44.1/48 kHz model".into(),
            state: if high_model_ready {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: high_model_error.unwrap_or_else(|| effective.high_sample_rate_model.clone()),
        });

        let low_model_error =
            Self::clearvoice_model_error(&model_root, &effective.low_sample_rate_model);
        let low_model_ready = low_model_error.is_none();
        checks.push(DenoiseCheck {
            id: "low-rate-model".into(),
            label: "Low-rate model".into(),
            state: if low_model_ready {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Warning
            },
            detail: low_model_error
                .map(|error| format!("{error} Low-rate files will skip denoising."))
                .unwrap_or_else(|| effective.low_sample_rate_model.clone()),
        });

        let device = if import_ready {
            self.resolved_device(&python, &effective.device)
        } else {
            Err("Validate the Python environment before selecting a processing device.".into())
        };
        checks.push(DenoiseCheck {
            id: "device".into(),
            label: "Processing device".into(),
            state: if device.is_ok() {
                DenoiseCheckState::Ready
            } else {
                DenoiseCheckState::Error
            },
            detail: device
                .as_ref()
                .map(|device| match device.as_str() {
                    "cpu" => "CPU is ready.".into(),
                    "cuda" => "Default CUDA GPU is ready.".into(),
                    value => format!("{value} is ready."),
                })
                .unwrap_or_else(|error| error.clone()),
        });

        let runtime_ready = python_exists && import_ready && worker_ready && device.is_ok();
        let ready = effective.enabled && runtime_ready && high_model_ready;
        let summary = if !effective.enabled {
            "Automatic denoising is off. Normalized audio remains fully editable.".into()
        } else if let Some(error) = self
            .config_error
            .read()
            .ok()
            .and_then(|error| error.clone())
        {
            error
        } else if ready && low_model_ready {
            "ClearVoice is ready for high-rate and low-rate audio.".into()
        } else if ready {
            "ClearVoice is ready for standard 44.1/48 kHz audio.".into()
        } else {
            "Complete the highlighted items to enable automatic denoising.".into()
        };

        DenoiseProviderStatus {
            provider_id: "clearvoice".into(),
            provider_name: "ClearVoice".into(),
            configured,
            effective,
            environment_overrides,
            models,
            devices,
            checks,
            ready,
            summary,
        }
    }

    fn write_config_file(config_path: &Path, config: &DenoiseConfig) -> Result<(), String> {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create the settings directory: {error}"))?;
        }
        let contents = serde_json::to_vec_pretty(config)
            .map_err(|error| format!("Unable to encode denoising settings: {error}"))?;
        let temporary = config_path.with_extension("json.tmp");
        fs::write(&temporary, contents)
            .map_err(|error| format!("Unable to write denoising settings: {error}"))?;
        fs::rename(&temporary, config_path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("Unable to save denoising settings: {error}")
        })
    }

    fn write_config(&self, config: &DenoiseConfig) -> Result<(), String> {
        Self::write_config_file(&self.config_path, config)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkerConfig {
    python: PathBuf,
    worker: PathBuf,
    model_root: PathBuf,
    device: String,
    persistent: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerRequest<'a> {
    id: u64,
    operation: &'static str,
    model: &'a str,
    input: &'a Path,
    output: &'a Path,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerResponse {
    id: Option<u64>,
    status: String,
    error: Option<String>,
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_reader: Option<JoinHandle<VecDeque<String>>>,
}

impl WorkerProcess {
    fn start(config: &WorkerConfig) -> Result<Self, String> {
        let mut command = Command::new(&config.python);
        command
            .args([
                config.worker.display().to_string(),
                "--serve".into(),
                "--device".into(),
                config.device.clone(),
            ])
            .current_dir(&config.model_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Unable to start ClearVoice worker: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("ClearVoice worker has no input channel.")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("ClearVoice worker has no output channel.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("ClearVoice worker has no error channel.")?;
        let stderr_reader = std::thread::spawn(move || {
            let mut recent = VecDeque::with_capacity(40);
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if recent.len() == 40 {
                    recent.pop_front();
                }
                recent.push_back(line);
            }
            recent
        });
        let mut process = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_reader: Some(stderr_reader),
        };
        let ready = match process.read_response() {
            Ok(ready) => ready,
            Err(error) => return Err(process.failure_with_stderr(error)),
        };
        if ready.status != "ready" {
            return Err(process.failure_with_stderr(format!(
                "ClearVoice worker did not become ready: {}",
                ready.error.unwrap_or(ready.status)
            )));
        }
        Ok(process)
    }

    fn read_response(&mut self) -> Result<WorkerResponse, String> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self
                .stdout
                .read_line(&mut line)
                .map_err(|error| format!("Unable to read ClearVoice worker output: {error}"))?;
            if read == 0 {
                return Err("ClearVoice worker stopped unexpectedly.".into());
            }
            let Some(payload) = line.trim_end().strip_prefix(WORKER_PROTOCOL_PREFIX) else {
                continue;
            };
            return serde_json::from_str(payload)
                .map_err(|error| format!("Invalid ClearVoice worker response: {error}"));
        }
    }

    fn enhance(
        &mut self,
        request_id: u64,
        model_name: &str,
        input: &Path,
        output: &Path,
    ) -> Result<(), WorkerFailure> {
        let request = WorkerRequest {
            id: request_id,
            operation: "enhance",
            model: model_name,
            input,
            output,
        };
        serde_json::to_writer(&mut self.stdin, &request).map_err(|error| {
            WorkerFailure::Transport(format!("Unable to encode ClearVoice task: {error}"))
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|()| self.stdin.flush())
            .map_err(|error| {
                WorkerFailure::Transport(format!("Unable to submit ClearVoice task: {error}"))
            })?;
        let response = self.read_response().map_err(WorkerFailure::Transport)?;
        if response.id != Some(request_id) {
            return Err(WorkerFailure::Transport(
                "ClearVoice worker returned an unexpected task ID.".into(),
            ));
        }
        match response.status.as_str() {
            "complete" => Ok(()),
            "error" => {
                Err(WorkerFailure::Task(response.error.unwrap_or_else(|| {
                    "ClearVoice task failed without details.".into()
                })))
            }
            status => Err(WorkerFailure::Transport(format!(
                "ClearVoice worker returned unexpected status {status}."
            ))),
        }
    }

    fn stop(&mut self) {
        let _ = self.stop_and_collect_stderr();
    }

    fn stop_and_collect_stderr(&mut self) -> VecDeque<String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stderr_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    }

    fn failure_with_stderr(&mut self, error: String) -> String {
        let details = self
            .stop_and_collect_stderr()
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n");
        if details.trim().is_empty() {
            error
        } else {
            format!("{error}: {}", details.trim())
        }
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

enum WorkerFailure {
    Task(String),
    Transport(String),
}

struct WorkerState {
    config: Option<WorkerConfig>,
    process: Option<WorkerProcess>,
}

struct ClearVoiceWorkerPool {
    state: Mutex<WorkerState>,
    next_request_id: AtomicU64,
}

impl ClearVoiceWorkerPool {
    fn new() -> Self {
        Self {
            state: Mutex::new(WorkerState {
                config: None,
                process: None,
            }),
            next_request_id: AtomicU64::new(1),
        }
    }

    fn enhance(
        &self,
        config: WorkerConfig,
        model_name: &str,
        input: &Path,
        output: &Path,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "ClearVoice worker state is unavailable.".to_string())?;
        if state.config.as_ref() != Some(&config) {
            state.process = None;
            state.config = Some(config.clone());
        }
        for attempt in 0..=1 {
            if state.process.is_none() {
                state.process = Some(WorkerProcess::start(&config)?);
            }
            let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            let result = state
                .process
                .as_mut()
                .expect("worker process initialized")
                .enhance(request_id, model_name, input, output);
            match result {
                Ok(()) => return Ok(()),
                Err(WorkerFailure::Task(error)) => {
                    return Err(format!("ClearVoice failed: {error}"))
                }
                Err(WorkerFailure::Transport(error)) if attempt == 0 => {
                    state.process = None;
                    let _ = fs::remove_file(output);
                    if error.is_empty() {
                        return Err("ClearVoice worker transport failed.".into());
                    }
                }
                Err(WorkerFailure::Transport(error)) => {
                    state.process = None;
                    return Err(error);
                }
            }
        }
        Err("ClearVoice worker failed unexpectedly.".into())
    }

    fn reset(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.process = None;
            state.config = None;
        }
    }
}

struct ClearVoiceSession {
    worker_pool: Arc<ClearVoiceWorkerPool>,
    worker_config: WorkerConfig,
    model_name: String,
    input_sample_rate: u32,
}

impl ClearVoiceSession {
    fn run_command(&self, input: &Path, output: &Path) -> Result<(), String> {
        if !self.worker_config.persistent {
            let result = Command::new(&self.worker_config.python)
                .args([
                    self.worker_config.worker.display().to_string(),
                    "--model".into(),
                    self.model_name.clone(),
                    "--input".into(),
                    input.display().to_string(),
                    "--output".into(),
                    output.display().to_string(),
                    "--device".into(),
                    self.worker_config.device.clone(),
                ])
                .current_dir(&self.worker_config.model_root)
                .output()
                .map_err(|error| format!("Unable to start ClearVoice: {error}"))?;
            return if result.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "ClearVoice failed: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                ))
            };
        }
        self.worker_pool
            .enhance(self.worker_config.clone(), &self.model_name, input, output)
    }
}

impl DenoiseSession for ClearVoiceSession {
    fn input_sample_rate(&self) -> u32 {
        self.input_sample_rate
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn enhance(&self, input: &Path, output: &Path) -> Result<(), String> {
        self.run_command(input, output)
    }
}

impl DenoiseProvider for ClearVoiceProvider {
    fn status(&self) -> DenoiseProviderStatus {
        self.build_status(self.configured())
    }

    fn inspect_config(&self, config: DenoiseConfig) -> DenoiseProviderStatus {
        self.clear_validation_caches();
        self.build_status(config)
    }

    fn save_config(&self, config: DenoiseConfig) -> Result<DenoiseProviderStatus, String> {
        let config = Self::normalize_config(config);
        self.write_config(&config)?;
        let mut current = self
            .config
            .write()
            .map_err(|_| "Denoising settings are unavailable.".to_string())?;
        *current = config;
        drop(current);
        if let Ok(mut error) = self.config_error.write() {
            *error = None;
        }
        self.worker_pool.reset();
        self.clear_validation_caches();
        Ok(self.status())
    }

    fn prepare(&self, source_sample_rate: u32) -> Result<Box<dyn DenoiseSession>, String> {
        if source_sample_rate == 0 {
            return Err("The audio stream has no sample rate.".into());
        }
        let configured = self.configured();
        let (effective, _) = Self::effective_config(&configured);
        if !effective.enabled {
            return Err("Automatic denoising is turned off.".into());
        }
        let python = PathBuf::from(&effective.executable_path);
        if !python.is_file() {
            return Err(
                "Choose the Python executable from a ClearVoice environment in Denoising settings."
                    .into(),
            );
        }
        if !self.python_has_clearvoice(&python) {
            return Err("The selected Python environment cannot import clearvoice.".into());
        }
        let custom_worker = std::env::var_os("SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER").is_some();
        let worker = Self::worker_path()
            .filter(|path| path.is_file())
            .ok_or("The ClearVoice adapter is missing from this installation.")?;
        let model_root = PathBuf::from(&effective.model_root);
        let (model_name, input_sample_rate) = if source_sample_rate >= 44_100 {
            (effective.high_sample_rate_model, 48_000)
        } else {
            (effective.low_sample_rate_model, 16_000)
        };
        if !Self::has_clearvoice_model(&model_root, &model_name) {
            return Err(format!(
                "Model {model_name} is not complete in the selected model library."
            ));
        }
        let device = self.resolved_device(&python, &effective.device)?;
        Ok(Box::new(ClearVoiceSession {
            worker_pool: self.worker_pool.clone(),
            worker_config: WorkerConfig {
                python,
                worker,
                model_root,
                device,
                persistent: !custom_worker,
            },
            model_name,
            input_sample_rate,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{ClearVoiceProvider, DenoiseConfig};
    use crate::denoise::DenoiseProvider;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("simple-audio-cut-{label}-{stamp}"))
    }

    #[test]
    fn recognizes_only_complete_model_directories() {
        let root = test_root("model-path");
        let model_dir = root.join("checkpoints/MossFormer2_SE_48K");
        fs::create_dir_all(&model_dir).expect("create model directory");
        fs::write(model_dir.join("last_best_checkpoint"), "model.pt\n")
            .expect("write model marker");
        assert_eq!(
            ClearVoiceProvider::clearvoice_model_error(&root, "MossFormer2_SE_48K"),
            Some(format!(
                "Missing checkpoint file: {}.",
                model_dir.join("model.pt").display()
            ))
        );
        assert!(!ClearVoiceProvider::has_clearvoice_model(
            &root,
            "MossFormer2_SE_48K"
        ));
        fs::write(model_dir.join("model.pt"), b"model").expect("write checkpoint");
        assert!(ClearVoiceProvider::has_clearvoice_model(
            &root,
            "MossFormer2_SE_48K"
        ));
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn accepts_a_root_checkpoints_folder_or_model_folder() {
        let root = test_root("model-root-normalize");
        let model = root.join("checkpoints/MossFormer2_SE_48K");
        fs::create_dir_all(&model).expect("create model directory");
        fs::write(model.join("last_best_checkpoint"), "model.pt").expect("write marker");

        assert_eq!(
            ClearVoiceProvider::normalize_model_root(root.to_str().unwrap()),
            root
        );
        assert_eq!(
            ClearVoiceProvider::normalize_model_root(root.join("checkpoints").to_str().unwrap()),
            root
        );
        assert_eq!(
            ClearVoiceProvider::normalize_model_root(model.to_str().unwrap()),
            root
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn extracts_device_json_after_runtime_startup_output() {
        let stdout = "runtime banner\nSIMPLE_AUDIO_CUT_DEVICES=[\"GPU 0\", \"GPU 1\"]\n";
        let payload = stdout
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("SIMPLE_AUDIO_CUT_DEVICES="))
            .expect("device payload");

        assert_eq!(
            serde_json::from_str::<Vec<String>>(payload).expect("parse devices"),
            vec!["GPU 0", "GPU 1"]
        );
    }

    #[test]
    fn persists_provider_configuration() {
        let root = test_root("denoise-config");
        let config_path = root.join("denoise.json");
        let provider = ClearVoiceProvider::new(config_path.clone());
        let configured = DenoiseConfig {
            enabled: false,
            executable_path: "/custom/python".into(),
            model_root: "/models".into(),
            device: "cpu".into(),
            high_sample_rate_model: "high".into(),
            low_sample_rate_model: "low".into(),
        };

        provider
            .save_config(configured.clone())
            .expect("save config");
        let reloaded = ClearVoiceProvider::new(config_path);

        assert_eq!(reloaded.configured(), configured);
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn migrates_the_legacy_runtime_and_recordings_override_to_clearvoice() {
        let root = test_root("legacy-denoise");
        let legacy = root.join("io.github.xander_lin.simple_audio_cut");
        let clearvoice = legacy.join("clearvoice");
        let python = clearvoice.join(".venv/bin/python");
        fs::create_dir_all(clearvoice.join("checkpoints")).expect("create checkpoints");
        fs::create_dir_all(python.parent().unwrap()).expect("create Python directory");
        fs::write(&python, b"python").expect("create Python executable");

        let (config, migrated) = ClearVoiceProvider::migrate_legacy_config(
            DenoiseConfig::default(),
            Some(python.clone()),
            Some(legacy.join("recordings")),
        );

        assert!(migrated);
        assert_eq!(config.executable_path, python.display().to_string());
        assert_eq!(config.model_root, clearvoice.display().to_string());
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn legacy_discovery_does_not_replace_saved_custom_paths() {
        let root = test_root("legacy-custom-denoise");
        let legacy = root.join("io.github.xander_lin.simple_audio_cut");
        fs::create_dir_all(legacy.join("clearvoice/checkpoints")).expect("create checkpoints");
        let configured = DenoiseConfig {
            executable_path: "/custom/python".into(),
            model_root: "/custom/models".into(),
            ..DenoiseConfig::default()
        };

        let (migrated, changed) = ClearVoiceProvider::migrate_legacy_config(
            configured.clone(),
            Some(legacy.join("clearvoice/.venv/bin/python")),
            Some(legacy.join("recordings")),
        );

        assert!(!changed);
        assert_eq!(migrated, configured);
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
