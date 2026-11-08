//! The one action a device panel has in flight: its row, the failure left by
//! the last one, and a generation so a timeout or an answer that arrives after
//! a newer action began is ignored. Each panel keeps its own timeout and
//! failure wording.

#[derive(Debug)]
pub struct ActionInFlight<A, F> {
    pub action: Option<A>,
    pub failure: Option<F>,
    generation: u64,
}

impl<A, F> Default for ActionInFlight<A, F> {
    fn default() -> Self {
        Self { action: None, failure: None, generation: 0 }
    }
}

impl<A, F> ActionInFlight<A, F> {
    /// The generation of the new action, or None while another runs. The
    /// previous failure is dropped.
    pub fn begin(&mut self, action: A) -> Option<u64> {
        if self.action.is_some() {
            return None;
        }
        self.action = Some(action);
        self.failure = None;
        self.generation += 1;
        Some(self.generation)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Ends the action `generation` began. `failure` turns it into the
    /// failure to show, or None when it succeeded. False when that action is
    /// no longer the one in flight.
    pub fn end(&mut self, generation: u64, failure: impl FnOnce(A) -> Option<F>) -> bool {
        if self.generation != generation {
            return false;
        }
        let Some(action) = self.action.take() else { return false };
        if let Some(failure) = failure(action) {
            self.failure = Some(failure);
        }
        true
    }

    /// Drops the action without a failure when `settled` says it is done.
    pub fn clear_if(&mut self, settled: impl FnOnce(&A) -> bool) {
        if self.action.as_ref().is_some_and(settled) {
            self.action = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_action_at_a_time_and_a_stale_end_is_ignored() {
        let mut f = ActionInFlight::<&str, &str>::default();
        let first = f.begin("a").unwrap();
        assert!(f.begin("b").is_none());
        assert!(f.end(first, |_| Some("late")));
        assert_eq!(f.failure, Some("late"));
        let second = f.begin("c").unwrap();
        assert!(f.failure.is_none());
        assert!(!f.end(first, |_| Some("stale")));
        assert!(f.end(second, |_| None));
        assert!(f.failure.is_none() && f.action.is_none());
    }
}
