use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitOutcome {
    Allowed,
    Limited,
}

#[derive(Debug)]
struct Bucket {
    started_at: Instant,
    count: u32,
}

#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn check(&self, key: &str, limit: u32, window: Duration) -> RateLimitOutcome {
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        prune_expired(&mut buckets, window);

        let Some(bucket) = buckets.get(key) else {
            return RateLimitOutcome::Allowed;
        };
        if bucket.started_at.elapsed() >= window || bucket.count < limit {
            RateLimitOutcome::Allowed
        } else {
            RateLimitOutcome::Limited
        }
    }

    pub fn record_failure(&self, key: &str, window: Duration) {
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        prune_expired(&mut buckets, window);

        let now = Instant::now();
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            started_at: now,
            count: 0,
        });
        if bucket.started_at.elapsed() >= window {
            bucket.started_at = now;
            bucket.count = 0;
        }
        bucket.count = bucket.count.saturating_add(1);
    }

    pub fn record_attempt(&self, key: &str, limit: u32, window: Duration) -> RateLimitOutcome {
        if self.check(key, limit, window) == RateLimitOutcome::Limited {
            return RateLimitOutcome::Limited;
        }
        self.record_failure(key, window);
        RateLimitOutcome::Allowed
    }

    pub fn clear(&self, key: &str) {
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        buckets.remove(key);
    }
}

fn prune_expired(buckets: &mut HashMap<String, Bucket>, window: Duration) {
    buckets.retain(|_, bucket| bucket.started_at.elapsed() < window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_after_threshold_until_cleared() {
        let limiter = RateLimiter::default();
        let window = Duration::from_secs(60);

        assert_eq!(
            limiter.record_attempt("login:user@example.com", 2, window),
            RateLimitOutcome::Allowed
        );
        assert_eq!(
            limiter.record_attempt("login:user@example.com", 2, window),
            RateLimitOutcome::Allowed
        );
        assert_eq!(
            limiter.record_attempt("login:user@example.com", 2, window),
            RateLimitOutcome::Limited
        );

        limiter.clear("login:user@example.com");
        assert_eq!(
            limiter.record_attempt("login:user@example.com", 2, window),
            RateLimitOutcome::Allowed
        );
    }
}
