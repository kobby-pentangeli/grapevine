//! Implements rate limiting.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Cleanup interval for removing stale peer buckets (5 minutes).
const CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

/// Maximum age for a peer bucket before cleanup (10 minutes).
const BUCKET_MAX_AGE: Duration = Duration::from_secs(600);

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

impl RateLimitConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.capacity == 0 {
            return Err("Rate limit capacity must be greater than 0".to_string());
        }
        if self.refill_rate == 0 {
            return Err("Rate limit refill_rate must be greater than 0".to_string());
        }
        Ok(())
    }
}

/// Simple token bucket rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    /// Rate limiting configuration
    config: RateLimitConfig,
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
    /// Create a new rate limiter from configuration.
    ///
    /// # Arguments
    /// * `config` - Rate limiting configuration
    ///
    /// # Panics
    /// Panics if the configuration is invalid.
    pub fn new(config: RateLimitConfig) -> Self {
        config.validate().expect("Invalid rate limit configuration");

        Self {
            config,
            buckets: HashMap::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Create a new rate limiter with explicit parameters.
    ///
    /// # Arguments
    /// * `capacity` - Maximum burst size (tokens)
    /// * `refill_rate` - Tokens added per second
    ///
    /// # Panics
    /// Panics if capacity or refill_rate is zero.
    pub fn with_params(capacity: u32, refill_rate: u32) -> Self {
        Self::new(RateLimitConfig {
            enabled: true,
            capacity,
            refill_rate,
        })
    }

    /// Get the current configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Check if a request from the given peer should be allowed.
    ///
    /// Returns `true` if the request is within the rate limit, `false` otherwise.
    pub fn allow_request(&mut self, peer: SocketAddr) -> bool {
        self.maybe_cleanup();

        let capacity = self.config.capacity;
        let refill_rate = self.config.refill_rate;

        let bucket = self.buckets.entry(peer).or_insert_with(|| TokenBucket {
            tokens: capacity as f64,
            last_refill: Instant::now(),
        });

        Self::refill_bucket(bucket, capacity, refill_rate);

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Refill tokens in a bucket based on elapsed time.
    fn refill_bucket(bucket: &mut TokenBucket, capacity: u32, refill_rate: u32) {
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();

        bucket.tokens += elapsed * refill_rate as f64;
        bucket.tokens = bucket.tokens.min(capacity as f64);
        bucket.last_refill = now;
    }

    /// Periodic cleanup of stale peer buckets.
    ///
    /// Removes buckets that haven't been used for more than `BUCKET_MAX_AGE`.
    /// This prevents unbounded memory growth for peers that disconnect.
    fn maybe_cleanup(&mut self) {
        if self.last_cleanup.elapsed() > CLEANUP_INTERVAL {
            let cutoff = Instant::now() - BUCKET_MAX_AGE;
            self.buckets.retain(|_, bucket| bucket.last_refill > cutoff);
            self.last_cleanup = Instant::now();
        }
    }

    /// Get the number of active peer buckets.
    #[cfg(test)]
    fn bucket_count(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn allows_within_limit() {
        let mut limiter = RateLimiter::with_params(10, 10);
        let peer = "127.0.0.1:8000".parse().unwrap();

        for _ in 0..10 {
            assert!(limiter.allow_request(peer), "Should allow within capacity");
        }
    }

    #[test]
    fn blocks_over_limit() {
        let mut limiter = RateLimiter::with_params(5, 5);
        let peer = "127.0.0.1:8000".parse().unwrap();

        for _ in 0..5 {
            assert!(limiter.allow_request(peer));
        }

        assert!(
            !limiter.allow_request(peer),
            "Should block when over capacity"
        );
    }

    #[test]
    fn refills() {
        let mut limiter = RateLimiter::with_params(2, 10);
        let peer = "127.0.0.1:8000".parse().unwrap();

        assert!(limiter.allow_request(peer));
        assert!(limiter.allow_request(peer));
        assert!(!limiter.allow_request(peer));

        thread::sleep(Duration::from_millis(200));

        assert!(limiter.allow_request(peer), "Should refill after delay");
    }

    #[test]
    fn per_peer() {
        let mut limiter = RateLimiter::with_params(2, 2);
        let peer1 = "127.0.0.1:8000".parse().unwrap();
        let peer2 = "127.0.0.1:8001".parse().unwrap();

        assert!(limiter.allow_request(peer1));
        assert!(limiter.allow_request(peer1));
        assert!(!limiter.allow_request(peer1));

        assert!(
            limiter.allow_request(peer2),
            "Peer2 should have separate bucket"
        );
        assert!(limiter.allow_request(peer2));
        assert!(!limiter.allow_request(peer2));
    }

    #[test]
    fn from_config() {
        let config = RateLimitConfig {
            enabled: true,
            capacity: 5,
            refill_rate: 10,
        };

        let mut limiter = RateLimiter::new(config.clone());
        assert_eq!(limiter.config().capacity, 5);
        assert_eq!(limiter.config().refill_rate, 10);

        let peer = "127.0.0.1:8000".parse().unwrap();
        for _ in 0..5 {
            assert!(limiter.allow_request(peer));
        }
        assert!(!limiter.allow_request(peer));
    }

    #[test]
    fn config_validation() {
        let invalid_capacity = RateLimitConfig {
            enabled: true,
            capacity: 0,
            refill_rate: 10,
        };
        assert!(invalid_capacity.validate().is_err());

        let invalid_refill = RateLimitConfig {
            enabled: true,
            capacity: 10,
            refill_rate: 0,
        };
        assert!(invalid_refill.validate().is_err());

        let valid = RateLimitConfig {
            enabled: true,
            capacity: 10,
            refill_rate: 5,
        };
        assert!(valid.validate().is_ok());
    }

    #[test]
    #[should_panic(expected = "Invalid rate limit configuration")]
    fn panics_on_invalid_config() {
        let invalid_config = RateLimitConfig {
            enabled: true,
            capacity: 0,
            refill_rate: 10,
        };
        RateLimiter::new(invalid_config);
    }

    #[test]
    fn bucket_cleanup() {
        let mut limiter = RateLimiter::with_params(10, 10);
        let peer1 = "127.0.0.1:8000".parse().unwrap();
        let peer2 = "127.0.0.1:8001".parse().unwrap();

        limiter.allow_request(peer1);
        limiter.allow_request(peer2);

        assert_eq!(limiter.bucket_count(), 2);

        limiter.last_cleanup = Instant::now() - CLEANUP_INTERVAL - Duration::from_secs(1);

        limiter.buckets.get_mut(&peer1).unwrap().last_refill =
            Instant::now() - BUCKET_MAX_AGE - Duration::from_secs(1);

        limiter.allow_request(peer2);

        assert_eq!(
            limiter.bucket_count(),
            1,
            "Stale bucket should be cleaned up"
        );
    }
}
