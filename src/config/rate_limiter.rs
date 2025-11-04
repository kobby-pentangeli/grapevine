//! Implements rate limiting.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Enable rate limiting
    pub enabled: bool,

    /// Maximum burst size (tokens)
    pub capacity: u32,

    /// Token refill rate (tokens per second)
    pub refill_rate: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capacity: 100,
            refill_rate: 50,
        }
    }
}

/// Simple token bucket rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum tokens (burst capacity)
    capacity: u32,
    /// Token refill rate (tokens per second)
    refill_rate: u32,
    /// Per-peer token buckets
    buckets: HashMap<SocketAddr, TokenBucket>,
    /// Last cleanup time
    last_cleanup: Instant,
}

#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Arguments
    /// * `capacity` - Maximum burst size (tokens)
    /// * `refill_rate` - Tokens added per second
    pub fn new(capacity: u32, refill_rate: u32) -> Self {
        Self {
            capacity,
            refill_rate,
            buckets: HashMap::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Check if a request from the given peer should be allowed.
    pub fn check_rate_limit(&mut self, peer: SocketAddr) -> bool {
        self.maybe_cleanup();

        let bucket = self.buckets.entry(peer).or_insert_with(|| TokenBucket {
            tokens: self.capacity as f64,
            last_refill: Instant::now(),
        });

        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();

        bucket.tokens += elapsed * self.refill_rate as f64;
        bucket.tokens = bucket.tokens.min(self.capacity as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Periodic cleanup of old buckets (every 5 minutes).
    fn maybe_cleanup(&mut self) {
        if self.last_cleanup.elapsed() > Duration::from_secs(300) {
            let cutoff = Instant::now() - Duration::from_secs(600);
            self.buckets.retain(|_, bucket| bucket.last_refill > cutoff);
            self.last_cleanup = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(10, 10);
        let peer = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();

        for _ in 0..10 {
            assert!(
                limiter.check_rate_limit(peer),
                "Should allow within capacity"
            );
        }
    }

    #[test]
    fn rate_limiter_blocks_over_limit() {
        let mut limiter = RateLimiter::new(5, 5);
        let peer = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();

        for _ in 0..5 {
            assert!(limiter.check_rate_limit(peer));
        }

        assert!(
            !limiter.check_rate_limit(peer),
            "Should block when over capacity"
        );
    }

    #[test]
    fn rate_limiter_refills() {
        let mut limiter = RateLimiter::new(2, 10);
        let peer = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();

        assert!(limiter.check_rate_limit(peer));
        assert!(limiter.check_rate_limit(peer));
        assert!(!limiter.check_rate_limit(peer));

        thread::sleep(Duration::from_millis(200));

        assert!(limiter.check_rate_limit(peer), "Should refill after delay");
    }

    #[test]
    fn rate_limiter_per_peer() {
        let mut limiter = RateLimiter::new(2, 2);
        let peer1 = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();
        let peer2 = "127.0.0.1:8001".parse::<SocketAddr>().unwrap();

        assert!(limiter.check_rate_limit(peer1));
        assert!(limiter.check_rate_limit(peer1));
        assert!(!limiter.check_rate_limit(peer1));

        assert!(
            limiter.check_rate_limit(peer2),
            "Peer2 should have separate bucket"
        );
        assert!(limiter.check_rate_limit(peer2));
        assert!(!limiter.check_rate_limit(peer2));
    }
}
