use std::time::Duration;

use that_limit_core::{Config, Policy};

pub trait ConfigExt {
    fn with_protected_quota(&self, quota: u64) -> Self;

    fn with_protected_reset_in(&self, reset_in: Duration) -> Self;

    fn with_protected_lease_size(&self, lease_size: u64) -> Self;
}

impl ConfigExt for Config {
    fn with_protected_quota(&self, quota: u64) -> Self {
        Self {
            protected: Policy {
                quota,
                ..self.protected.clone()
            },
            ..self.clone()
        }
    }

    fn with_protected_reset_in(&self, reset_in: Duration) -> Self {
        Self {
            protected: Policy {
                reset_in,
                ..self.protected.clone()
            },
            ..self.clone()
        }
    }

    fn with_protected_lease_size(&self, lease_size: u64) -> Self {
        Self {
            protected: Policy {
                lease_size,
                ..self.protected.clone()
            },
            ..self.clone()
        }
    }
}
