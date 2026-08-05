#[cfg(feature = "clearvoice-denoise")]
mod clearvoice;
#[cfg(all(feature = "native-audio", not(feature = "clearvoice-denoise")))]
mod disabled;
#[cfg(any(test, not(feature = "native-audio")))]
mod mock;

#[cfg(feature = "clearvoice-denoise")]
pub use clearvoice::ClearVoiceProvider;
#[cfg(all(feature = "native-audio", not(feature = "clearvoice-denoise")))]
pub use disabled::DisabledDenoiseProvider;
#[cfg(any(test, not(feature = "native-audio")))]
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
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
