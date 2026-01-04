use dashmap::DashMap;

use crate::cfg::Config;

#[derive(Debug)]
pub struct Store {
    pub store: DashMap<String, u128>,
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        Self {
            store: DashMap::with_capacity(10000),
            config,
        }
    }

    pub fn add_public(&self, s: &str) {
        self.store.insert(s.to_string(), self.config.public.quota);
    }

    pub fn add_protected(&self, s: &str) {
        self.store
            .insert(s.to_string(), self.config.protected.quota);
    }

    pub fn consume(&self, s: &str) {
        if let Some(mut b) = self.store.get_mut(s) {
            if *b > 0 {
                *b -= 1; // decrement by 1
            }
        }
    }
}
