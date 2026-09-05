//! Shared time-state rules. All-day and cancelled events are not "next meeting".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    AllDay,
    Ended,
    Ongoing,
    Upcoming,
    Cancelled,
}

pub fn classify(start: i32, duration: i32, all_day: bool, cancelled: bool, now: i32) -> Timing {
    if cancelled {
        Timing::Cancelled
    } else if all_day {
        Timing::AllDay
    } else if start.saturating_add(duration.max(1)) <= now {
        Timing::Ended
    } else if start <= now {
        Timing::Ongoing
    } else {
        Timing::Upcoming
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn morning_evening_and_boundaries_use_current_time() {
        assert_eq!(classify(540, 60, false, false, 500), Timing::Upcoming);
        assert_eq!(classify(540, 60, false, false, 540), Timing::Ongoing);
        assert_eq!(classify(540, 60, false, false, 600), Timing::Ended);
        assert_eq!(classify(1200, 60, false, false, 1300), Timing::Ended);
        assert_eq!(classify(0, 1440, true, false, 600), Timing::AllDay);
        assert_eq!(classify(700, 60, false, true, 600), Timing::Cancelled);
    }
}
