//! Arming state machine for one hot corner. No clock of its own: every
//! transition takes the caller's `now_ms`, so the whole rule set is testable
//! head-on.
//!
//! The problem it exists for: firing a corner maps the action's own surface
//! (the lock plate, the screensaver overlay) over the corner, which takes the
//! pointer with it. When that surface goes away the compositor hands the
//! pointer straight back to a corner the cursor never really left, and the
//! enter that arrives is indistinguishable, on its own, from a fresh
//! approach. Re-arming on that enter relocks the session the instant it is
//! unlocked, under a parked cursor.
//!
//! The rule: after the action ends, the corner stays disarmed until the
//! pointer has genuinely left the corner and a quiet period has passed since
//! the end.
//!
//! The action's end is the moment its covering surface unmaps, not the moment
//! the action reports itself inactive: the screensaver keeps covering the
//! corner through its fade, and a cooldown started before that runs out under
//! the cover. `on_action_end` takes `pointer_in_corner`, computed by the
//! caller from the compositor's cursor position at that moment. Hover state
//! is no substitute: it reads false while a surface covers the corner, so a
//! cursor parked there looked like one that had left, and the hand-back enter
//! after the cooldown fired the corner again.
//!
//! A hand-back is caught twice over:
//!
//!   1. `pointer_in_corner` at the end. If the cursor is still on the corner,
//!      no leave has happened yet and none is invented.
//!   2. An enter arriving inside the cooldown, which is what the hand-back
//!      looks like when the compositor's position was stale at the unmap. Such
//!      an enter cancels the leave that preceded it instead of firing.
//!
//! Moving out and back in after the cooldown is an ordinary approach again and
//! fires normally.

use super::corners::Action;

/// The quiet period after an action ends, in ms. The same 400 as the shell's
/// longest surface reveal, on the reasoning that a corner must not fire again
/// until the surface it fired is fully off screen and the compositor has
/// settled its pointer focus. A constant here rather than the motion token
/// because disabling motion zeroes that token, and this is a correctness
/// guard, not an animation.
pub const REARM_COOLDOWN_MS: i64 = 400;

/// Whether an action reports its own end back to the shell, which is what
/// starts the cooldown at the right moment rather than at the fire.
///
/// "lock" does only while this shell owns the surface. An external locker
/// (`lock.command`: hyprlock, swaylock, loginctl) owns the session on its own
/// terms and never reports back, so its corner takes the same path a launcher
/// action does: the end is the fire, and the leave requirement is the whole
/// guard. That is weaker (a locker held up for a minute outlives the
/// cooldown), and it is the most this shell can honestly say about a process
/// it only spawned.
///
/// "screensaver" locks the session too and draws the screensaver inside the
/// lock, so it ends where the lock does: at the unlock, and only with this
/// shell's own lock. The screensaver being dismissed back to the password
/// card is not the end, the corner is still covered.
///
/// "overview" toggles: the corner that opened it closes it again, so its
/// cooldown starts at the fire, as the launcher's does.
pub fn reports_end(action: &Action, external_lock: bool) -> bool {
    match action {
        Action::Lock | Action::Screensaver => !external_lock,
        Action::None | Action::Overview | Action::Launcher(_) => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmState {
    pub armed: bool,
    /// The action's own surface is up.
    pub running: bool,
    pub ended_at_ms: i64,
    /// The pointer has left the corner since the action ended.
    pub left: bool,
}

pub fn initial() -> ArmState {
    ArmState {
        armed: true,
        running: false,
        ended_at_ms: 0,
        left: false,
    }
}

fn state(armed: bool, running: bool, ended_at_ms: i64, left: bool) -> ArmState {
    ArmState {
        armed,
        running,
        ended_at_ms,
        left,
    }
}

/// The state a corner surface should come up in, given what its action is
/// doing right now. These windows are not permanent: the model behind them is
/// rebuilt whenever the output list or the config changes, so a screen waking,
/// a monitor being plugged in or a settings.json save while the session is
/// locked destroys every corner and builds a fresh one. Coming up plainly
/// armed there is the same relock by another road, because the enter the
/// compositor sends the new surface at unlock lands on a corner with no memory
/// of having fired. `ended_at_ms` is the controller's record of when this
/// action last ended, 0 if it never has.
pub fn adopt(now_ms: i64, action_running: bool, ended_at_ms: i64) -> ArmState {
    if action_running {
        return state(false, true, 0, false);
    }
    if ended_at_ms > 0 && now_ms - ended_at_ms < REARM_COOLDOWN_MS {
        return state(false, false, ended_at_ms, false);
    }
    initial()
}

/// May an enter start the dwell? Either the corner is plainly armed, or its
/// action is over, the pointer has left since, and the quiet period is up.
pub fn is_armed(s: ArmState, now_ms: i64) -> bool {
    if s.armed {
        return true;
    }
    if s.running {
        return false;
    }
    s.left && now_ms - s.ended_at_ms >= REARM_COOLDOWN_MS
}

/// The corner fired. `action_reports_end` decides whether the cooldown waits
/// for the action's own end or starts here.
pub fn on_fire(_s: ArmState, now_ms: i64, action_reports_end: bool) -> ArmState {
    if action_reports_end {
        return state(false, true, 0, false);
    }
    state(false, false, now_ms, false)
}

/// The action's covering surface unmapped (the lock plate is gone, the
/// screensaver's fade is over). `pointer_in_corner` is the compositor's
/// cursor position at that moment tested against the corner's own square:
/// false means the cursor is somewhere else entirely and the leave
/// requirement is already met.
pub fn on_action_end(s: ArmState, now_ms: i64, pointer_in_corner: bool) -> ArmState {
    if !s.running {
        return s;
    }
    state(false, false, now_ms, !pointer_in_corner)
}

pub fn on_enter(s: ArmState, now_ms: i64) -> ArmState {
    if is_armed(s, now_ms) {
        return initial();
    }
    // Inside the cooldown this is the compositor handing the pointer back, so
    // whatever leave preceded it belonged to the same unmap and stops counting.
    state(false, s.running, s.ended_at_ms, false)
}

pub fn on_exit(s: ArmState) -> ArmState {
    if s.armed {
        return s;
    }
    // The action's own surface taking the pointer is not the pointer leaving,
    // so a leave while it is up says nothing.
    if s.running {
        return s;
    }
    state(false, false, s.ended_at_ms, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the surface does on an enter: read whether the corner is armed,
    /// then fold the enter in. The dwell starts only when it was armed.
    fn enter(s: &mut ArmState, now_ms: i64) -> bool {
        let armed = is_armed(*s, now_ms);
        *s = on_enter(*s, now_ms);
        armed
    }

    // A corner that never fires is just armed, and a dwell cut short by the
    // pointer moving away leaves it that way.
    #[test]
    fn a_fresh_corner_is_armed() {
        let mut s = initial();
        assert!(is_armed(s, 0));
        s = on_exit(s);
        assert!(is_armed(s, 0));
        s = on_enter(s, 0);
        assert!(is_armed(s, 0));
    }

    #[test]
    fn firing_disarms_until_the_action_ends() {
        let mut s = on_fire(initial(), 1000, true);
        assert!(!is_armed(s, 1000));
        // The lock plate taking the pointer is not the pointer leaving.
        s = on_exit(s);
        assert!(!is_armed(s, 60000));
    }

    // The bug: unlocking with the cursor parked in the corner. The action
    // ends, the compositor hands the pointer back, and that enter must not
    // fire the corner again.
    #[test]
    fn the_hand_back_after_an_unlock_does_not_re_arm() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_exit(s);
        s = on_action_end(s, 5000, true);
        assert!(!is_armed(s, 5000));
        s = on_enter(s, 5005);
        assert!(!is_armed(s, 5005));
        // Still parked a long while later: nothing has left, so nothing arms.
        assert!(!is_armed(s, 60000));
    }

    // The same hand-back with a stale pointer read at the unmap, so the end
    // reported the cursor outside the corner even though it never moved. The
    // enter inside the cooldown is what catches it.
    #[test]
    fn an_enter_inside_the_cooldown_cancels_the_leave_before_it() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_action_end(s, 5000, false);
        s = on_exit(s);
        s = on_enter(s, 5000 + REARM_COOLDOWN_MS - 1);
        assert!(!is_armed(s, 60000));
    }

    #[test]
    fn leaving_for_real_then_returning_fires_again() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_action_end(s, 5000, true);
        s = on_enter(s, 5005);
        s = on_exit(s);
        assert!(is_armed(s, 5000 + REARM_COOLDOWN_MS));
        s = on_enter(s, 9000);
        assert!(is_armed(s, 9000));
    }

    // The cursor was somewhere else when the action ended (the password was
    // typed, the pointer moved). Nothing has to leave, only the cooldown.
    #[test]
    fn a_pointer_already_elsewhere_needs_only_the_cooldown() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_action_end(s, 5000, false);
        assert!(!is_armed(s, 5000 + REARM_COOLDOWN_MS - 1));
        assert!(is_armed(s, 5000 + REARM_COOLDOWN_MS));
    }

    #[test]
    fn the_cooldown_is_measured_from_the_end_not_the_fire() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_exit(s);
        s = on_action_end(s, 30000, false);
        assert!(!is_armed(s, 30000 + REARM_COOLDOWN_MS - 1));
        assert!(is_armed(s, 30000 + REARM_COOLDOWN_MS));
    }

    // A launcher action string and an external locker both report nothing
    // back, so their cooldown runs from the fire and the leave is the whole
    // guard.
    #[test]
    fn an_action_with_no_reported_end_still_needs_a_leave() {
        let mut s = on_fire(initial(), 1000, false);
        assert!(!is_armed(s, 1000 + REARM_COOLDOWN_MS));
        s = on_enter(s, 1000 + REARM_COOLDOWN_MS);
        assert!(!is_armed(s, 60000));
        s = on_exit(s);
        assert!(is_armed(s, 60000));
    }

    // Nothing of ours is running, so an end belonging to some other trigger
    // (a keybind lock, an idle screensaver) cannot disarm a corner.
    #[test]
    fn an_end_with_nothing_pending_changes_nothing() {
        let s = on_action_end(initial(), 1000, true);
        assert!(is_armed(s, 1000));
        let fired = on_fire(initial(), 1000, true);
        let ended = on_action_end(fired, 5000, true);
        let again = on_action_end(ended, 9000, true);
        assert_eq!(again.ended_at_ms, 5000);
    }

    #[test]
    fn which_actions_report_their_own_end() {
        assert!(reports_end(&Action::Lock, false));
        assert!(!reports_end(&Action::Lock, true));
        assert!(reports_end(&Action::Screensaver, false));
        assert!(!reports_end(&Action::Screensaver, true));
        assert!(!reports_end(
            &Action::Launcher("@ipc:theme.toggleMode".into()),
            false
        ));
        assert!(!reports_end(
            &Action::Launcher("hyprctl dispatch workspace 1".into()),
            false
        ));
    }

    // The screensaver corner locks: dismissing the screensaver back to the
    // password card leaves the corner covered, so only the unlock ends it.
    #[test]
    fn the_screensaver_corner_waits_for_the_unlock() {
        let fired = on_fire(initial(), 1000, reports_end(&Action::Screensaver, false));
        assert!(!is_armed(fired, 60_000));
        let unlocked = on_action_end(fired, 60_000, true);
        assert!(!is_armed(on_enter(unlocked, 60_050), 60_050));
        let external = on_fire(initial(), 1000, reports_end(&Action::Screensaver, true));
        assert_eq!(external.ended_at_ms, 1000);
    }

    // A corner surface rebuilt while its own action is up (an output waking,
    // a settings.json save) must not come up armed, or the enter it gets at
    // unlock is the same relock by another road.
    #[test]
    fn a_surface_built_while_the_action_runs_adopts_it() {
        let mut s = adopt(5000, true, 0);
        assert!(!is_armed(s, 60000));
        s = on_action_end(s, 6000, true);
        s = on_enter(s, 6050);
        assert!(!is_armed(s, 60000));
        s = on_exit(s);
        assert!(is_armed(s, 60000));
    }

    #[test]
    fn a_surface_built_inside_the_cooldown_adopts_the_rest_of_it() {
        let mut s = adopt(5000, false, 5000 - REARM_COOLDOWN_MS + 100);
        assert!(!is_armed(s, 5000));
        // Nothing has left since, so the cooldown running out is not enough.
        assert!(!is_armed(s, 60000));
        s = on_exit(s);
        assert!(is_armed(s, 60000));
    }

    #[test]
    fn a_surface_built_with_nothing_pending_is_plainly_armed() {
        assert!(is_armed(adopt(60000, false, 0), 60000));
        assert!(is_armed(adopt(60000, false, 5000), 60000));
    }

    #[test]
    fn the_cooldown_is_the_documented_number() {
        assert_eq!(REARM_COOLDOWN_MS, 400);
    }

    // The screensaver corner after a dismiss with the cursor parked in it.
    // The end is the unmap at the close of the 400ms fade, not the moment the
    // action dropped to inactive, and the pointer is read off the compositor
    // there. With the cursor still in the corner the hand-back enter after the
    // cooldown must not fire; only a real exit and a later enter does.
    #[test]
    fn a_dismiss_with_the_cursor_parked_stays_disarmed_until_a_real_exit() {
        let mut s = on_fire(initial(), 1000, true);
        assert!(!is_armed(s, 1000));

        // Covered: the surface holds the pointer through the whole run, so a
        // leave and an enter while it is up change nothing.
        s = on_exit(s);
        assert!(!enter(&mut s, 2000));
        assert!(!is_armed(s, 3000));

        // Dismissed at 4000, the fade runs to 4400 and the surface unmaps. The
        // cursor is still in the corner at that moment.
        s = on_exit(s);
        assert!(!is_armed(s, 4200));
        s = on_action_end(s, 4400, true);

        // The compositor hands the pointer back after the cooldown has run
        // out, which is what fired the corner before.
        let hand_back = 4400 + REARM_COOLDOWN_MS + 50;
        assert!(!enter(&mut s, hand_back));
        assert!(!is_armed(s, hand_back + 60000));

        // A real exit, then an approach after the cooldown: fires.
        s = on_exit(s);
        let approach = hand_back + 1000;
        assert!(enter(&mut s, approach));
        assert!(is_armed(s, approach));
    }

    // The hover-driven input the bug shipped with: the end read the cursor as
    // outside the corner while a surface covered it, so the hand-back enter
    // after the cooldown fired. Kept as the reason the caller passes the
    // compositor's position.
    #[test]
    fn a_pointer_misread_as_outside_fires_on_the_hand_back() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_action_end(s, 4400, false);
        assert!(enter(&mut s, 4400 + REARM_COOLDOWN_MS + 50));
    }

    // An exit and re-enter that both land inside the cooldown are still the
    // hand-back; the next approach after it is a real one.
    #[test]
    fn an_exit_and_return_inside_the_cooldown_does_not_fire() {
        let mut s = on_fire(initial(), 1000, true);
        s = on_action_end(s, 4400, true);
        s = on_exit(s);
        assert!(!enter(&mut s, 4400 + 100));
        assert!(!is_armed(s, 4400 + 60000));
    }
}
