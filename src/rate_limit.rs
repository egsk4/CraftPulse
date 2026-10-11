use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_UPDATES: usize = 4;
const WINDOW: Duration = Duration::from_secs(20);

#[derive(Debug, Default)]
pub struct RpcRateLimiter {
    updates: VecDeque<Instant>,
}

impl RpcRateLimiter {
    pub fn try_acquire(&mut self, now: Instant) -> bool {
        while self
            .updates
            .front()
            .is_some_and(|previous| now.saturating_duration_since(*previous) >= WINDOW)
        {
            self.updates.pop_front();
        }
        if self.updates.len() >= MAX_UPDATES {
            return false;
        }
        self.updates.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_activity_to_four_updates_per_twenty_seconds() {
        let start = Instant::now();
        let mut limiter = RpcRateLimiter::default();
        for offset in [0, 5, 10, 15] {
            assert!(limiter.try_acquire(start + Duration::from_secs(offset)));
        }
        assert!(!limiter.try_acquire(start + Duration::from_secs(19)));
    }

    #[test]
    fn reopens_capacity_when_the_oldest_update_leaves_the_window() {
        let start = Instant::now();
        let mut limiter = RpcRateLimiter::default();
        for offset in [0, 5, 10, 15] {
            assert!(limiter.try_acquire(start + Duration::from_secs(offset)));
        }
        assert!(limiter.try_acquire(start + Duration::from_secs(20)));
    }

    #[test]
    fn allows_a_later_update_after_a_blocked_attempt() {
        let start = Instant::now();
        let mut limiter = RpcRateLimiter::default();
        for offset in [0, 5, 10, 15] {
            limiter.try_acquire(start + Duration::from_secs(offset));
        }
        assert!(!limiter.try_acquire(start + Duration::from_secs(18)));
        assert!(limiter.try_acquire(start + Duration::from_secs(20)));
    }
}
