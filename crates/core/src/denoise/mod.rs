#[cfg(feature = "clearvoice-denoise")]
mod clearvoice;
#[cfg(all(feature = "native-audio", not(feature = "clearvoice-denoise")))]
mod disabled;
mod mock;

#[cfg(feature = "clearvoice-denoise")]
pub use clearvoice::ClearVoiceProvider;
#[cfg(all(feature = "native-audio", not(feature = "clearvoice-denoise")))]
pub use disabled::DisabledDenoiseProvider;
pub use mock::MockDenoiseProvider;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseConfig {
    pub enabled: bool,
    pub executable_path: String,
    pub model_root: String,
    pub device: String,
    pub high_sample_rate_model: String,
    pub low_sample_rate_model: String,
}

impl Default for DenoiseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            executable_path: String::new(),
            model_root: String::new(),
            device: "auto".into(),
            high_sample_rate_model: "MossFormer2_SE_48K".into(),
            low_sample_rate_model: "FRCRN_SE_16K".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseModelInfo {
    pub id: String,
    pub sample_rate: Option<u32>,
    pub ready: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseDeviceInfo {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DenoiseCheckState {
    Ready,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseCheck {
    pub id: String,
    pub label: String,
    pub state: DenoiseCheckState,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseProviderStatus {
    pub provider_id: String,
    pub provider_name: String,
    pub configured: DenoiseConfig,
    pub effective: DenoiseConfig,
    pub environment_overrides: Vec<String>,
    pub models: Vec<DenoiseModelInfo>,
    pub devices: Vec<DenoiseDeviceInfo>,
    pub checks: Vec<DenoiseCheck>,
    pub ready: bool,
    pub summary: String,
}

impl DenoiseProviderStatus {
    /// True when a real provider is wired into this build.
    ///
    /// The disabled stand-in is still a valid status (and must keep the rest
    /// of the editor usable), but it can never denoise anything, so callers
    /// use this to distinguish "switched off by the user" from "not
    /// installed".
    pub fn has_provider(&self) -> bool {
        self.provider_id != "disabled"
    }

    /// True when an automatic run can start right now: the user enabled it and
    /// the environment passed every blocking check.
    pub fn can_denoise_automatically(&self) -> bool {
        self.effective.enabled && self.ready
    }

    /// Why automatic denoising cannot run, preferring a concrete failed check
    /// over the generic summary. Always returns something the user can act on.
    pub fn blocking_reason(&self) -> String {
        if !self.has_provider() {
            return "This build does not include a denoising provider.".into();
        }
        if !self.effective.enabled {
            return "Automatic denoising is off. You can denoise this take manually.".into();
        }
        if let Some(check) = self
            .checks
            .iter()
            .find(|check| check.state == DenoiseCheckState::Error)
        {
            return format!("{}: {}", check.label, check.detail);
        }
        self.summary.clone()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseAvailability {
    pub available: bool,
    pub provider_name: String,
    pub model_name: Option<String>,
    pub reason: Option<String>,
}

pub trait DenoiseSession: Send {
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    fn input_sample_rate(&self) -> u32;
    fn model_name(&self) -> &str;
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    fn enhance(&self, input: &Path, output: &Path) -> Result<(), String>;
}

pub trait DenoiseProvider: Send + Sync {
    fn status(&self) -> DenoiseProviderStatus;
    fn inspect_config(&self, config: DenoiseConfig) -> DenoiseProviderStatus;
    fn save_config(&self, config: DenoiseConfig) -> Result<DenoiseProviderStatus, String>;
    fn prepare(&self, source_sample_rate: u32) -> Result<Box<dyn DenoiseSession>, String>;

    fn availability(&self, source_sample_rate: u32) -> DenoiseAvailability {
        match self.prepare(source_sample_rate) {
            Ok(session) => DenoiseAvailability {
                available: true,
                provider_name: self.status().provider_name,
                model_name: Some(session.model_name().to_string()),
                reason: None,
            },
            Err(error) => DenoiseAvailability {
                available: false,
                provider_name: self.status().provider_name,
                model_name: None,
                reason: Some(error),
            },
        }
    }
}
