use std::collections::HashMap;

use crate::cfg::Config;

#[derive(Debug)]
pub struct Store {
    pub store: HashMap<String, u128>,
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        Self {
            store: HashMap::with_capacity(10000),
            config,
        }
    }

    pub fn add_public(&mut self, s: &str) {
        if self.store.contains_key(s) {
            self.store.insert(s.to_string(), self.config.public.quota);
        }
    }

    pub fn add_protected(&mut self, s: &str) {
        if self.store.contains_key(s) {
            self.store
                .insert(s.to_string(), self.config.protected.quota);
        }
    }

    pub fn consume(&mut self, s: &str) {
        if let Some(b) = self.store.get(s)
            && b.gt(&0)
        {
            self.store.insert(s.to_string(), b - 1);
        }
    }
}
