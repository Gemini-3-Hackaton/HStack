use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::VecDeque;
use std::time::{Instant, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub requests_per_minute: u32,
    pub tokens_per_minute: u32,
}

#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    // Simple sliding window using request timestamps
    history_sec: Arc<Mutex<VecDeque<Instant>>>,
    history_min: Arc<Mutex<VecDeque<Instant>>>,
    // For tokens, we'd need to track amounts. Leaving out for now or implementing simple count.
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            history_sec: Arc::new(Mutex::new(VecDeque::new())),
            history_min: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn check(&self) -> bool {
        let now = Instant::now();
        let mut sec_history = self.history_sec.lock().await;
        let mut min_history = self.history_min.lock().await;

        // Clean up old entries
        while let Some(&t) = sec_history.front() {
            if now.duration_since(t) > Duration::from_secs(1) {
                sec_history.pop_front();
            } else {
                break;
            }
        }
        while let Some(&t) = min_history.front() {
            if now.duration_since(t) > Duration::from_secs(60) {
                min_history.pop_front();
            } else {
                break;
            }
        }

        if sec_history.len() >= self.config.requests_per_second as usize {
            return false;
        }
        if min_history.len() >= self.config.requests_per_minute as usize {
            return false;
        }

        sec_history.push_back(now);
        min_history.push_back(now);
        true
    }
}
