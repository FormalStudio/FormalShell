//! Time as the surfaces word it.

/// now/Nm/Nh/Nd ago, each unit's own upper bound exclusive so an exact hour
/// reads "1h ago" rather than "60m ago".
pub fn rel_time(now_ms: i64, arrived_at_ms: i64) -> String {
    let diff = (now_ms - arrived_at_ms).max(0);
    let minute = 60 * 1000;
    let hour = 60 * minute;
    let day = 24 * hour;
    if diff < minute {
        "now".into()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else {
        format!("{}d ago", diff / day)
    }
}

#[cfg(test)]
mod tests {
    use super::rel_time;

    #[test]
    fn rel_time_under_a_minute_is_now() {
        assert_eq!(rel_time(1000, 1000), "now");
        assert_eq!(rel_time(60999, 1000), "now");
    }

    #[test]
    fn rel_time_minute_boundary() {
        assert_eq!(rel_time(61000, 1000), "1m ago");
    }

    #[test]
    fn rel_time_minutes_just_under_an_hour() {
        assert_eq!(rel_time(1000 + 59 * 60 * 1000, 1000), "59m ago");
    }

    #[test]
    fn rel_time_hour_boundary_reads_hours_not_sixty_minutes() {
        assert_eq!(rel_time(1000 + 60 * 60 * 1000, 1000), "1h ago");
    }

    #[test]
    fn rel_time_hours_just_under_a_day() {
        assert_eq!(rel_time(1000 + 23 * 60 * 60 * 1000, 1000), "23h ago");
    }

    #[test]
    fn rel_time_day_boundary_reads_days_not_twentyfour_hours() {
        assert_eq!(rel_time(1000 + 24 * 60 * 60 * 1000, 1000), "1d ago");
    }

    #[test]
    fn rel_time_multiple_days() {
        assert_eq!(rel_time(1000 + 3 * 24 * 60 * 60 * 1000, 1000), "3d ago");
    }

    #[test]
    fn rel_time_clamps_future_arrival_to_now() {
        assert_eq!(rel_time(1000, 5000), "now");
    }
}
