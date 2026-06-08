//! Implements rate limiting.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Error;

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
    /// # Errors
    /// Returns [`Error::Config`] if the configuration is invalid (zero capacity
    /// or refill rate).
    pub fn try_new(config: RateLimitConfig) -> crate::Result<Self> {
        config.validate().map_err(Error::Config)?;

        Ok(Self {
            config,
            buckets: HashMap::new(),
            last_cleanup: Instant::now(),
        })
    }

    /// Create a new rate limiter with explicit parameters.
    ///
    /// # Arguments
    /// * `capacity` - Maximum burst size (tokens)
    /// * `refill_rate` - Tokens added per second
    ///
    /// # Errors
    /// Returns [`Error::Config`] if `capacity` or `refill_rate` is zero.
    pub fn try_with_params(capacity: u32, refill_rate: u32) -> crate::Result<Self> {
        Self::try_new(RateLimitConfig {
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
            tokens: f64::from(capacity),
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

        bucket.tokens += elapsed * f64::from(refill_rate);
        bucket.tokens = bucket.tokens.min(f64::from(capacity));
        bucket.last_refill = now;
    }

    /// Periodic cleanup of stale peer buckets.
    ///
    /// Removes buckets that haven't been used for more than `BUCKET_MAX_AGE`.
    /// This prevents unbounded memory growth for peers that disconnect.
    fn maybe_cleanup(&mut self) {
        if self.last_cleanup.elapsed() > CLEANUP_INTERVAL {
            let now = Instant::now();
            self.buckets
                .retain(|_, bucket| bucket.last_refill.elapsed() < BUCKET_MAX_AGE);
            self.last_cleanup = now;
        }
    }

    /// Get the number of active peer buckets.
    #[cfg(test)]
    fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Force cleanup for testing purposes with a custom max age.
    #[cfg(test)]
    fn cleanup_with_max_age(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.buckets
            .retain(|_, bucket| bucket.last_refill.elapsed() < max_age);
        self.last_cleanup = now;
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn allows_within_limit() {
        let mut limiter = RateLimiter::try_with_params(10, 10).unwrap();
        let peer = "127.0.0.1:8000".parse().unwrap();

        for _ in 0..10 {
            assert!(limiter.allow_request(peer), "Should allow within capacity");
        }
    }

    #[test]
    fn blocks_over_limit() {
        let mut limiter = RateLimiter::try_with_params(5, 5).unwrap();
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
        let mut limiter = RateLimiter::try_with_params(2, 10).unwrap();
        let peer = "127.0.0.1:8000".parse().unwrap();

        assert!(limiter.allow_request(peer));
        assert!(limiter.allow_request(peer));
        assert!(!limiter.allow_request(peer));

        thread::sleep(Duration::from_millis(200));

        assert!(limiter.allow_request(peer), "Should refill after delay");
    }

    #[test]
    fn per_peer() {
        let mut limiter = RateLimiter::try_with_params(2, 2).unwrap();
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

        let mut limiter = RateLimiter::try_new(config.clone()).unwrap();
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
    fn rejects_invalid_config() {
        let invalid_config = RateLimitConfig {
            enabled: true,
            capacity: 0,
            refill_rate: 10,
        };
        assert!(RateLimiter::try_new(invalid_config).is_err());
    }

    #[test]
    fn bucket_cleanup() {
        const TEST_BUCKET_MAX_AGE: Duration = Duration::from_millis(100);

        let mut limiter = RateLimiter::try_with_params(10, 10).unwrap();
        let peer1 = "127.0.0.1:8000".parse().unwrap();
        let peer2 = "127.0.0.1:8001".parse().unwrap();

        // Create bucket for peer1
        limiter.allow_request(peer1);
        assert_eq!(limiter.bucket_count(), 1);

        // Wait for peer1's bucket to age beyond MAX_AGE
        thread::sleep(Duration::from_millis(120));

        // Create bucket for peer2 (fresh)
        limiter.allow_request(peer2);
        assert_eq!(limiter.bucket_count(), 2);

        // Trigger cleanup with test-specific max age
        // This tests the cleanup logic using a shorter timeout than production
        limiter.cleanup_with_max_age(TEST_BUCKET_MAX_AGE);

        // Only peer2 should remain (peer1's bucket is > 100ms old, peer2 is fresh)
        assert_eq!(
            limiter.bucket_count(),
            1,
            "Stale bucket should be cleaned up"
        );

        // Verify peer2 is the one that remains
        assert!(limiter.buckets.contains_key(&peer2));
        assert!(!limiter.buckets.contains_key(&peer1));
    }
}
