//! The practice timer: one day's session, accumulated across pauses.
//! Pure state machine - the UI supplies the clock, so this is testable.

/// A running or paused practice session, bound to the day it was started on.
#[derive(Debug, Clone)]
pub struct PracticeTimer {
    pub day_id: i64,
    /// Seconds banked by segments that have already been paused.
    banked: f64,
    /// Clock reading when the current segment began; `None` while paused.
    running_since: Option<f64>,
}

impl PracticeTimer {
    pub fn started(day_id: i64, now: f64) -> Self {
        Self {
            day_id,
            banked: 0.0,
            running_since: Some(now),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    /// Seconds on the clock, counting the segment in progress.
    pub fn elapsed(&self, now: f64) -> f64 {
        self.banked + self.running_since.map_or(0.0, |since| segment(since, now))
    }

    pub fn pause(&mut self, now: f64) {
        if let Some(since) = self.running_since.take() {
            self.banked += segment(since, now);
        }
    }

    pub fn resume(&mut self, now: f64) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    /// Whole minutes to log, to the nearest minute.
    pub fn minutes(&self, now: f64) -> i64 {
        (self.elapsed(now) / 60.0).round() as i64
    }
}

/// Length of one run segment. A clock that reads backwards - a machine resumed
/// from sleep, say - must never subtract from time already practised.
fn segment(since: f64, now: f64) -> f64 {
    (now - since).max(0.0)
}

/// `mm:ss` for the timer row and the status bar.
pub fn format_clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as i64;
    format!("{:02}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_accumulates_across_a_pause_and_resume() {
        let mut t = PracticeTimer::started(1, 100.0);
        assert_eq!(t.elapsed(130.0), 30.0);

        t.pause(130.0);
        assert!(!t.is_running());
        // Paused time does not count, however long the app sits there.
        assert_eq!(t.elapsed(400.0), 30.0);

        t.resume(400.0);
        assert_eq!(t.elapsed(430.0), 60.0);
    }

    #[test]
    fn pause_and_resume_are_both_idempotent() {
        let mut t = PracticeTimer::started(1, 0.0);
        t.pause(60.0);
        t.pause(120.0);
        assert_eq!(t.elapsed(180.0), 60.0, "a second pause banks nothing extra");

        t.resume(180.0);
        t.resume(240.0);
        assert_eq!(t.elapsed(240.0), 120.0, "a second resume does not restart");
    }

    #[test]
    fn a_backwards_clock_never_loses_practised_time() {
        let mut t = PracticeTimer::started(1, 500.0);
        assert_eq!(t.elapsed(400.0), 0.0);

        t.pause(400.0);
        assert_eq!(t.elapsed(400.0), 0.0);
    }

    #[test]
    fn minutes_round_to_the_nearest_minute() {
        let t = PracticeTimer::started(1, 0.0);
        assert_eq!(t.minutes(29.0), 0, "under half a minute logs nothing");
        assert_eq!(t.minutes(31.0), 1);
        assert_eq!(t.minutes(90.0), 2);
        assert_eq!(t.minutes(3600.0), 60);
    }

    #[test]
    fn clock_is_zero_padded_minutes_and_seconds() {
        assert_eq!(format_clock(0.0), "00:00");
        assert_eq!(format_clock(9.6), "00:09");
        assert_eq!(format_clock(605.0), "10:05");
        assert_eq!(format_clock(-5.0), "00:00");
    }
}
