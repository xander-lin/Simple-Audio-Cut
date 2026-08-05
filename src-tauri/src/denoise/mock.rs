use super::{
    DenoiseCheck, DenoiseCheckState, DenoiseConfig, DenoiseDeviceInfo, DenoiseModelInfo,
    DenoiseProvider, DenoiseProviderStatus, DenoiseSession,
};
use std::path::Path;
use std::sync::RwLock;

pub struct MockDenoiseProvider {
    config: RwLock<DenoiseConfig>,
}

impl MockDenoiseProvider {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(DenoiseConfig::default()),
        }
    }

    fn ready_status(configured: DenoiseConfig) -> DenoiseProviderStatus {
        let enabled = configured.enabled;
        DenoiseProviderStatus {
            provider_id: "mock".into(),
            provider_name: "Mock denoising".into(),
            effective: configured.clone(),
            configured,
            environment_overrides: Vec::new(),
            models: vec![DenoiseModelInfo {
                id: "mock-model".into(),
                sample_rate: Some(48_000),
                ready: true,
                detail: "Test model".into(),
            }],
            devices: vec![DenoiseDeviceInfo {
                id: "cpu".into(),
                label: "CPU".into(),
            }],
            checks: vec![DenoiseCheck {
                id: "provider".into(),
                label: "Mock provider".into(),
                state: DenoiseCheckState::Ready,
                detail: "Ready".into(),
            }],
            ready: enabled,
            summary: if enabled {
                "Mock denoising is ready.".into()
            } else {
                "Mock denoising is disabled.".into()
            },
        }
    }

    fn configured(&self) -> DenoiseConfig {
        self.config
            .read()
            .map(|config| config.clone())
            .unwrap_or_default()
    }
}

struct MockDenoiseSession;

impl DenoiseSession for MockDenoiseSession {
    fn input_sample_rate(&self) -> u32 {
        48_000
    }

    fn model_name(&self) -> &str {
        "mock-model"
    }

    fn enhance(&self, input: &Path, output: &Path) -> Result<(), String> {
        std::fs::copy(input, output)
            .map(|_| ())
            .map_err(|error| format!("Unable to create mock denoising output: {error}"))
    }
}

impl DenoiseProvider for MockDenoiseProvider {
    fn status(&self) -> DenoiseProviderStatus {
        Self::ready_status(self.configured())
    }

    fn inspect_config(&self, config: DenoiseConfig) -> DenoiseProviderStatus {
        Self::ready_status(config)
    }

    fn save_config(&self, config: DenoiseConfig) -> Result<DenoiseProviderStatus, String> {
        *self
            .config
            .write()
            .map_err(|_| "Mock denoising settings are unavailable.".to_string())? = config;
        Ok(self.status())
    }

    fn prepare(&self, _source_sample_rate: u32) -> Result<Box<dyn DenoiseSession>, String> {
        if !self.configured().enabled {
            return Err("Mock denoising is disabled.".into());
        }
        Ok(Box::new(MockDenoiseSession))
    }
}

#[cfg(test)]
mod tests {
    use super::MockDenoiseProvider;
    use crate::denoise::DenoiseProvider;

    #[test]
    fn saved_configuration_controls_mock_availability() {
        let provider = MockDenoiseProvider::new();
        let mut config = provider.status().configured;
        config.enabled = false;

        let status = provider.save_config(config).expect("save mock config");

        assert!(!status.ready);
        assert!(provider.prepare(48_000).is_err());
    }
}
