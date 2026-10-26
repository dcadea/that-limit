use std::time::Duration;

use log::trace;
use that_limit_cache::{Key, Redis, Result, Script};

use crate::bucket;

impl Key for bucket::Id {
    fn to_key(&self) -> String {
        match self {
            Self::Public(ip) => format!("ip:{ip}"),
            Self::Protected(sub) => format!("sub:{sub}"),
        }
    }
}

const LEASE_SCRIPT: &str = r"
    local key = KEYS[1]
    local lease_size = tonumber(ARGV[1])
    local quota = tonumber(ARGV[2])
    local default_ttl = tonumber(ARGV[3])

    local current = redis.call('GET', key)

    if not current then
        local remaining = quota - lease_size
        redis.call('SETEX', key, default_ttl, remaining)
        return {lease_size, default_ttl, redis.call('PEXPIRETIME', key)}
    end

    local tokens = tonumber(current)
    if tokens <= 0 then
        return {0, redis.call('TTL', key), redis.call('PEXPIRETIME', key)}
    end

    local to_lease = math.min(tokens, lease_size)
    redis.call('DECRBY', key, to_lease)
    local ttl = redis.call('TTL', key)

    return {to_lease, ttl, redis.call('PEXPIRETIME', key)}
";

/// Takes up to `size` tokens for `id` from the current window,
/// opening a new window of `ttl` with `quota` tokens if there is none.
///
/// Returns leased tokens, time left in the window and the window id
/// (key expiry as unix time in ms, see `PEXPIRETIME`).
pub async fn lease(
    redis: &Redis,
    id: &bucket::Id,
    size: u64,
    quota: u64,
    ttl: Duration,
) -> Result<(u64, Duration, i64)> {
    trace!("LEASE -> {id}");

    let (leased, ttl_secs, window): (u64, u64, i64) = redis
        .eval(
            Script::new(LEASE_SCRIPT)
                .key(id)
                .arg(size)
                .arg(quota)
                .arg(ttl.as_secs()),
        )
        .await?;

    let ttl = if ttl_secs > 0 {
        Duration::from_secs(ttl_secs)
    } else {
        ttl
    };

    Ok((leased, ttl, window))
}

// Return tokens only to the window they were leased from: a plain INCRBY
// would recreate an expired key without TTL or top up a newer window.
const RELEASE_SCRIPT: &str = r"
    local key = KEYS[1]
    local tokens = tonumber(ARGV[1])
    local window = tonumber(ARGV[2])

    if redis.call('PEXPIRETIME', key) ~= window then
        return 0
    end

    redis.call('INCRBY', key, tokens)
    return 1
";

/// Returns unspent tokens for `id` to the window they were leased from.
/// `false` if that window is gone and the tokens were dropped.
pub async fn release(redis: &Redis, id: &bucket::Id, tokens: u64, window: i64) -> Result<bool> {
    trace!("RELEASE -> {id}");

    let released: u8 = redis
        .eval(Script::new(RELEASE_SCRIPT).key(id).arg(tokens).arg(window))
        .await?;

    Ok(released == 1)
}
