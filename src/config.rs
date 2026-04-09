use std::time::Duration;

use crate::ReplayError;

pub struct MirrorConfig {
    pub sample_rate: f64,
    pub channel_buffer_size: usize,
    pub max_concurrent_requests: usize,
    pub request_timeout: Duration,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        Self {
            sample_rate: 0.10,
            channel_buffer_size: 1024,
            max_concurrent_requests: 50,
            request_timeout: Duration::from_secs(5),
        }
    }
}

impl MirrorConfig {
    pub fn validate(&self) -> Result<(), ReplayError> {
        if !(0.0..=1.0).contains(&self.sample_rate) {
            return Err(ReplayError::InvalidConfig(
                "sample_rate must be between 0.0 and 1.0".into(),
            ));
        }
        if self.channel_buffer_size == 0 {
            return Err(ReplayError::InvalidConfig(
                "channel_buffer_size must be > 0".into(),
            ));
        }
        if self.max_concurrent_requests == 0 {
            return Err(ReplayError::InvalidConfig(
                "max_concurrent_requests must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = MirrorConfig::default();
        assert!((c.sample_rate - 0.10).abs() < f64::EPSILON);
        assert_eq!(c.channel_buffer_size, 1024);
        assert_eq!(c.max_concurrent_requests, 50);
    }

    #[test]
    fn valid_config() {
        MirrorConfig::default().validate().unwrap();
    }

    #[test]
    fn sample_rate_out_of_range() {
        let mut c = MirrorConfig::default();
        c.sample_rate = 1.5;
        assert!(c.validate().is_err());
        c.sample_rate = -0.1;
        assert!(c.validate().is_err());
    }

    #[test]
    fn zero_buffer_rejected() {
        let mut c = MirrorConfig::default();
        c.channel_buffer_size = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn zero_concurrency_rejected() {
        let mut c = MirrorConfig::default();
        c.max_concurrent_requests = 0;
        assert!(c.validate().is_err());
    }
}
