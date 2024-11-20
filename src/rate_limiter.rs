use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RateLimiter {
    requests: VecDeque<Instant>,
    window: Duration,
    max_requests: usize,
}

impl RateLimiter {
    pub fn new(window_secs: u64, max_requests: usize) -> Self {
        Self {
            requests: VecDeque::new(),
            window: Duration::from_secs(window_secs),
            max_requests,
        }
    }

    pub async fn wait_if_needed(&mut self) {
        let now = Instant::now();

        // Remove old requests outside the window
        while let Some(request_time) = self.requests.front() {
            if now.duration_since(*request_time) > self.window {
                self.requests.pop_front();
            } else {
                break;
            }
        }

        // If at capacity, wait until we can make another request
        if self.requests.len() >= self.max_requests {
            if let Some(oldest) = self.requests.front() {
                let wait_time = self.window - now.duration_since(*oldest);
                tokio::time::sleep(wait_time).await;
            }
        }

        self.requests.push_back(now);
    }
}
