//! Arm, then confirm: a first press on a control that ends something arms
//! it and asks, a second press on the same one goes ahead. The launcher's
//! Shutdown and Reboot rows, a process's End, the lock's buttons and quick
//! settings' power rows all ask this way.

/// What a first press armed, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Armed<K>(Option<K>);

impl<K> Default for Armed<K> {
    fn default() -> Self {
        Armed(None)
    }
}

impl<K: PartialEq> Armed<K> {
    /// A press on `key`: true when it was armed already, which disarms
    /// it and goes ahead; otherwise it arms, in place of anything else.
    pub fn press(&mut self, key: K) -> bool {
        if self.0.as_ref() == Some(&key) {
            self.0 = None;
            return true;
        }
        self.0 = Some(key);
        false
    }

    /// Arms `key` with no question asked: the press that confirms is still
    /// to come.
    pub fn arm(&mut self, key: K) {
        self.0 = Some(key);
    }

    pub fn is(&self, key: &K) -> bool {
        self.0.as_ref() == Some(key)
    }

    pub fn get(&self) -> Option<&K> {
        self.0.as_ref()
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Disarms unless `on` is what is armed: the cursor moved off it.
    pub fn keep(&mut self, on: Option<&K>) {
        if self.0.as_ref() != on {
            self.0 = None;
        }
    }
}

/// What an armed control says: `Confirm <what>?`.
pub fn ask(what: &str) -> String {
    format!("Confirm {what}?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_press_on_the_same_key_goes_ahead() {
        let mut a = Armed::default();
        assert!(!a.press("reboot"));
        assert!(a.is(&"reboot"));
        assert!(!a.press("poweroff"), "another key arms in its place");
        assert!(a.press("poweroff"));
        assert_eq!(a.get(), None);
        a.arm("reboot");
        a.keep(Some(&"reboot"));
        assert!(a.is(&"reboot"));
        a.keep(None);
        assert_eq!(a.get(), None);
    }

    #[test]
    fn an_armed_control_asks_after_its_noun() {
        assert_eq!(ask("Shutdown"), "Confirm Shutdown?", "a launcher row");
    }
}
