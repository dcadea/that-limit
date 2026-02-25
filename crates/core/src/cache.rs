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
