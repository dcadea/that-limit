pub trait Key {
    fn to_key(&self) -> String;
}

// #[derive(Debug)]
// pub struct Key<K: ToString> {
//     k: K,
// }

// impl<K: ToString> Key<K> {
//     pub fn new(k: K) -> Self {
//         Self { k }
//     }
// }

// impl<K> redis::ToRedisArgs for Key<K>
// where
//     K: ToString,
// {
//     fn write_redis_args<W>(&self, out: &mut W)
//     where
//         W: ?Sized + redis::RedisWrite,
//     {
//         self.k.to_string().write_redis_args(out)
//     }
// }

// impl<K: ToString> redis::ToSingleRedisArg for Key<K> {}
