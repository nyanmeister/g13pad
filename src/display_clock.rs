// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent telemetry and presentation clocks. See docs/health-architecture.md.
use std::time::{Duration, Instant};

/// The software's default sampling cadence, not an LCD hardware limit.
pub(crate) const TELEMETRY_INTERVAL: Duration = Duration::from_millis(100);
/// Reported G13 display ceiling: 10 ms per frame, up to 100 FPS.
pub(crate) const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct Clock {
    sample_at: Instant,
    frame_at: Instant,
}
impl Clock {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            sample_at: now,
            frame_at: now,
        }
    }
    pub(crate) fn sample_due(&mut self, now: Instant) -> bool {
        if now < self.sample_at {
            return false;
        }
        self.sample_at = now + TELEMETRY_INTERVAL;
        true
    }
    pub(crate) fn frame_due(&mut self, now: Instant, interval: Duration) -> bool {
        if now < self.frame_at {
            return false;
        }
        self.frame_at = now + interval.max(MIN_FRAME_INTERVAL);
        true
    }
    pub(crate) fn wait(&self, now: Instant) -> Duration {
        self.sample_at
            .min(self.frame_at)
            .saturating_duration_since(now)
    }
    /// Feed sampling is owned by the watcher; this loop only schedules presentation.
    pub(crate) fn frame_wait(&self, now: Instant) -> Duration {
        self.frame_at.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_frames_do_not_multiply_samples_or_replay_missed_frames() {
        let start = Instant::now();
        let mut clock = Clock::new(start);
        let mut samples = 0;
        let mut frames = 0;
        for n in 0..100 {
            let now = start + Duration::from_millis(n * 10);
            samples += usize::from(clock.sample_due(now));
            frames += usize::from(clock.frame_due(now, MIN_FRAME_INTERVAL));
        }
        assert_eq!((samples, frames), (10, 100));
        let slow = start + Duration::from_secs(3);
        assert!(clock.sample_due(slow));
        assert!(clock.frame_due(slow, Duration::ZERO));
        assert!(!clock.sample_due(slow));
        assert!(!clock.frame_due(slow, Duration::ZERO));
        assert_eq!(clock.wait(slow), MIN_FRAME_INTERVAL);
    }
}
