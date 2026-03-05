// TODO: see https://datatracker.ietf.org/doc/draft-ietf-httpapi-ratelimit-headers/
pub const RATE_LIMIT: &str = "ratelimit";

pub fn rate_limit(tokens_left: u64) -> String {
    format!("r={tokens_left}")
}
