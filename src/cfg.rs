use std::fs;

use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    Protected,
    Public,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Criteria {
    Sub,
    Ip,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct BucketCfg {
    criteria: Criteria,
    pub quota: u128,
    reset_in: u128,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub sync_every: u8,
    pub protected: BucketCfg,
    pub public: BucketCfg,
}
