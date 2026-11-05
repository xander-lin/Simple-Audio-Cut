use super::{
    DenoiseAvailability, DenoiseCheck, DenoiseCheckState, DenoiseConfig, DenoiseDeviceInfo,
    DenoiseProvider, DenoiseProviderStatus, DenoiseSession,
};

pub struct DisabledDenoiseProvider;

impl DisabledDenoiseProvider {
    pub fn new() -> Self {
        Self
    }

    fn unavailable_status(configured: DenoiseConfig) -> DenoiseProviderStatus {
        let mut effective = configured.clone();
        effective.enabled = false;
        DenoiseProviderStatus {
            provider_id: "disabled".into(),
            provider_name: "No denoising provider".into(),
            configured,
            effective,
            environment_overrides: Vec::new(),
            models: Vec::new(),
            devices: vec![DenoiseDeviceInfo {
                id: "cpu".into(),
                label: "CPU".into(),
            }],
            checks: vec![DenoiseCheck {
                id: "provider".into(),
                label: "Denoising module".into(),
                state: DenoiseCheckState::Warning,
                detail: "This build does not include a denoising provider.".into(),
            }],
            ready: false,
            summary: "Denoising is not included in this build.".into(),
        }
    }
}

impl DenoiseProvider for DisabledDenoiseProvider {
    fn status(&self) -> DenoiseProviderStatus {
        Self::unavailable_status(DenoiseConfig::default())
    }

    fn inspect_config(&self, config: DenoiseConfig) -> DenoiseProviderStatus {
        Self::unavailable_status(config)
    }

    fn save_config(&self, config: DenoiseConfig) -> Result<DenoiseProviderStatus, String> {
        Ok(Self::unavailable_status(config))
    }

    fn prepare(&self, _source_sample_rate: u32) -> Result<Box<dyn DenoiseSession>, String> {
        Err("Denoising is not included in this build.".into())
    }

    fn availability(&self, _source_sample_rate: u32) -> DenoiseAvailability {
        DenoiseAvailability {
            available: false,
            provider_name: "No denoising provider".into(),
            model_name: None,
            reason: Some("Denoising is not included in this build.".into()),
        }
    }
}
