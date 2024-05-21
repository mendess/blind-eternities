use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    sync::OnceLock,
    time::{Duration, SystemTime},
};

use mappable_rc::Marc;
use tokio::sync::Mutex;

use crate::metrics;

struct CacheEntry {
    t: Marc<dyn Any + Send + Sync>,
    created_at: SystemTime,
    duration: Duration,
}

impl CacheEntry {
    fn new<T: Send + Sync + 'static>(t: Marc<T>, duration: Duration) -> Self {
        Self {
            t: Marc::map(t, |t| t as _),
            created_at: SystemTime::now(),
            duration,
        }
    }

    fn get<T>(&self) -> Option<Marc<T>> {
        (self.created_at.elapsed().unwrap_or_default() < self.duration)
            .then(|| Marc::map(self.t.clone(), |t| t.downcast_ref().unwrap()))
            .or_else(|| {
                tracing::trace!(
                    created_at = ?self.created_at,
                    duration = ?self.duration,
                    ty = std::any::type_name::<T>(),
                    "cache expired "
                );
                None
            })
    }
}

type Cache = HashMap<(TypeId, String), CacheEntry>;

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(Default::default)
}

pub async fn get_or_init<F, Fut, T, E>(
    key: &str,
    default: F,
    duration: Duration,
) -> Result<Marc<T>, E>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    T: Send + Sync + 'static,
{
    tracing::trace!(ty = std::any::type_name::<T>(), key, "looking up in cache");
    let tid = TypeId::of::<T>();
    {
        let cache = cache().lock().await;
        if let Some(t) = cache
            .get(&(tid, key.to_string()))
            .and_then(|entry| entry.get())
        {
            metrics::cache_hit();
            return Ok(t);
        }
        metrics::cache_miss();
    }
    let new_t = Marc::new(default().await?);
    let mut cache = cache().lock().await;
    cache.insert(
        (tid, key.to_owned()),
        CacheEntry::new(new_t.clone(), duration),
    );
    Ok(new_t)
}
