use that_limit_cache::Key;

use crate::bucket;

impl Key for bucket::Id {
    fn to_key(&self) -> String {
        match self {
            Self::Public(ip) => format!("ip:{ip}"),
            Self::Protected(sub) => format!("sub:{sub}"),
        }
    }
}

// pub struct Lease<K> {
//     key: K,
//     size: u64,
//     quota: u64,
//     ttl: Duration,
// }

// impl<K> Lease<K> {
//     pub const fn new(key: K, size: u64, quota: u64, ttl: Duration) -> Self {
//         Self {
//             key,
//             size,
//             quota,
//             ttl,
//         }
//     }
// }

// const LEASE_SCRIPT: &str = r"
//     local key = KEYS[1]
//     local lease_size = tonumber(ARGV[1])
//     local quota = tonumber(ARGV[2])
//     local default_ttl = tonumber(ARGV[3])

//     local current = redis.call('GET', key)

//     if not current then
//         local remaining = quota - lease_size
//         redis.call('SETEX', key, default_ttl, remaining)
//         return {lease_size, default_ttl}
//     end

//     local tokens = tonumber(current)
//     if tokens <= 0 then
//         return {0, redis.call('TTL', key)}
//     end

//     local to_lease = math.min(tokens, lease_size)
//     redis.call('DECRBY', key, to_lease)
//     local ttl = redis.call('TTL', key)

//     return {to_lease, ttl}
// ";

// impl<K> Action for Lease<K>
// where
//     K: that_limit_cache::Key + Sync + Debug + ToString,
// {
//     type Output = (u64, Duration);

//     async fn execute(self, con: &mut redis::aio::ConnectionManager) -> super::Result<Self::Output> {
//         let key = self.key;
//         trace!("LEASE -> {key:?}");

//         let (leased, ttl_secs): (u64, u64) = redis::Script::new(LEASE_SCRIPT)
//             .key(&key)
//             .arg(self.size)
//             .arg(self.quota)
//             .arg(self.ttl.as_secs())
//             .invoke_async(con)
//             .await?;

//         let ttl = if ttl_secs > 0 {
//             Duration::from_secs(ttl_secs)
//         } else {
//             self.ttl
//         };

//         Ok((leased, ttl))
//     }
// }
