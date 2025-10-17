use std::sync::atomic::{AtomicU64, Ordering};

pub mod ws;

#[derive(PartialEq, Eq, Debug, Clone, Copy, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    pub fn next() -> Self {
        static GENERATION: AtomicU64 = AtomicU64::new(0);

        Self(GENERATION.fetch_add(1, Ordering::SeqCst))
    }
}
