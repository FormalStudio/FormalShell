use std::time::Duration;

/// A retry delay that doubles from `floor` to `ceiling`, back to `floor`
/// once something works.
#[derive(Clone, Debug)]
pub struct Backoff {
    floor: Duration,
    ceiling: Duration,
    next: Duration,
}

impl Backoff {
    pub const fn new(floor: Duration, ceiling: Duration) -> Self {
        Self { floor, ceiling, next: floor }
    }

    /// The delay to wait now; the one after is twice it, up to the ceiling.
    pub fn delay(&mut self) -> Duration {
        let now = self.next;
        self.next = (now * 2).min(self.ceiling);
        now
    }

    pub fn reset(&mut self) {
        self.next = self.floor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_to_the_ceiling_and_resets_to_the_floor() {
        let mut b = Backoff::new(Duration::from_secs(2), Duration::from_secs(10));
        let waits: Vec<u64> = (0..5).map(|_| b.delay().as_secs()).collect();
        assert_eq!(waits, [2, 4, 8, 10, 10]);
        b.reset();
        assert_eq!(b.delay(), Duration::from_secs(2));
    }
}
