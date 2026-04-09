use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Default, Clone)]
pub struct Counters {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    mirrored: AtomicU64,
    failed: AtomicU64,
    dropped: AtomicU64,
}

impl Counters {
    pub fn increment_mirrored(&self) {
        self.inner.mirrored.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_failed(&self) {
        self.inner.failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_dropped(&self) {
        self.inner.dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn mirrored(&self) -> u64 {
        self.inner.mirrored.load(Ordering::Relaxed)
    }

    pub fn failed(&self) -> u64 {
        self.inner.failed.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_and_read() {
        let c = Counters::default();
        c.increment_mirrored();
        c.increment_mirrored();
        c.increment_failed();
        c.increment_dropped();
        c.increment_dropped();
        c.increment_dropped();
        assert_eq!(c.mirrored(), 2);
        assert_eq!(c.failed(), 1);
        assert_eq!(c.dropped(), 3);
    }

    #[test]
    fn clone_shares_state() {
        let c1 = Counters::default();
        let c2 = c1.clone();
        c1.increment_mirrored();
        assert_eq!(c2.mirrored(), 1);
        c2.increment_failed();
        assert_eq!(c1.failed(), 1);
    }
}
