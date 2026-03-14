use that_limit_core::{Config, Policy};

pub trait ConfigExt {
    #[must_use]
    fn with_domain(&self, domain: impl Into<String>) -> Self;

    #[must_use]
    fn with_protected_quota(&self, quota: u64) -> Self;

    #[must_use]
    fn with_protected_lease_size(&self, lease_size: u64) -> Self;
}

impl ConfigExt for Config {
    fn with_domain(&self, domain: impl Into<String>) -> Self {
        Self {
            domains: vec![domain.into()],
            ..self.clone()
        }
    }

    fn with_protected_quota(&self, quota: u64) -> Self {
        Self {
            protected: Policy {
                quota,
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
