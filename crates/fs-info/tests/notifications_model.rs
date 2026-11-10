use std::sync::Mutex;

use fs_info::notifications::*;
use fs_types::NotificationId;

static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A readable name for a server id, so the assertions keep reading as `a`, `b`.
fn id(name: &str) -> NotificationId {
    let mut names = NAMES.lock().unwrap();
    let at = names.iter().position(|n| n == name).unwrap_or_else(|| {
        names.push(name.to_owned());
        names.len() - 1
    });
    NotificationId::Server(at as u32)
}

fn name(id: NotificationId) -> String {
    let NotificationId::Server(at) = id else { panic!("{id}") };
    NAMES.lock().unwrap()[at as usize].clone()
}

fn notif(name: &str, urgency: u8) -> Notif {
    Notif {
        id: id(name),
        app_name: "TestApp".into(),
        summary: format!("Summary {name}"),
        body: format!("Body {name}"),
        urgency: Urgency::try_from(urgency).unwrap(),
        ..Notif::default()
    }
}

fn summary(mut n: Notif, s: &str) -> Notif {
    n.summary = s.into();
    n
}

fn from_notify_send(mut n: Notif) -> Notif {
    n.sender_is_notify_send = true;
    n
}

fn ids(entries: &[Entry]) -> String {
    entries.iter().map(|e| name(e.id)).collect::<Vec<_>>().join(",")
}

fn add_at(s: &mut State, n: Notif, now: i64) {
    s.add(&n, now, &AddOpts::default());
}

fn add_timeout(s: &mut State, n: Notif, now: i64, timeout_ms: i64) {
    s.add(&n, now, &AddOpts { timeout_ms: Some(timeout_ms), ..AddOpts::default() });
}

fn dnd() -> State {
    let mut s = State::new();
    s.set_dnd(true);
    s
}

fn patch_summary(s: &str) -> Patch {
    Patch { summary: Some(s.into()), ..Patch::default() }
}

#[test]
fn initial_state_shape() {
    let s = State::new();
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 0);
    assert_eq!(s.past.len(), 0);
    assert!(!s.dnd);
    assert_eq!(s.next_expiry, None);
}

#[test]
fn bypasses_dnd_true_only_for_critical_notify_send() {
    assert!(bypasses_dnd(&from_notify_send(notif("a", 2))));
}

#[test]
fn bypasses_dnd_false_for_critical_chat_app() {
    // A chat app abusing urgency=critical must NOT bypass DND.
    assert!(!bypasses_dnd(&notif("a", 2)));
}

#[test]
fn bypasses_dnd_false_for_notify_send_at_normal_urgency() {
    assert!(!bypasses_dnd(&from_notify_send(notif("a", 1))));
}

#[test]
fn bypasses_dnd_true_for_critical_local() {
    let mut n = notif("a", 2);
    n.local = true;
    assert!(bypasses_dnd(&n));
}

#[test]
fn bypasses_dnd_false_for_local_at_normal_urgency() {
    let mut n = notif("a", 1);
    n.local = true;
    assert!(!bypasses_dnd(&n));
}

#[test]
fn add_dnd_on_non_bypassing_routes_to_pending() {
    let mut s = dnd();
    add_at(&mut s, notif("a", 1), 1000);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.pending[0].id, id("a"));
    assert_eq!(s.pending[0].seen_at, None);
}

#[test]
fn add_dnd_on_bypassing_critical_routes_to_popup() {
    let mut s = dnd();
    add_at(&mut s, from_notify_send(notif("a", 2)), 1000);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.pending.len(), 0);
}

#[test]
fn add_normal_urgency_gets_default_timeout_expiry() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    assert_eq!(s.popups[0].expires_at, Some(7000)); // 1000 + 6000ms default
}

#[test]
fn add_critical_urgency_is_sticky() {
    let mut s = State::new();
    add_at(&mut s, from_notify_send(notif("a", 2)), 1000);
    assert_eq!(s.popups[0].expires_at, Some(0));
}

#[test]
fn add_custom_timeout_via_opts() {
    let mut s = State::new();
    add_timeout(&mut s, notif("a", 0), 1000, 2000);
    assert_eq!(s.popups[0].expires_at, Some(3000));
}

#[test]
fn add_entry_shape_carries_arrival_time_and_null_seen() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1234);
    let e = &s.popups[0];
    assert_eq!(e.app_name, "TestApp");
    assert_eq!(e.summary, "Summary a");
    assert_eq!(e.arrived_at, 1234);
    assert_eq!(e.seen_at, None);
}

#[test]
fn popup_cap_overflow_pushes_oldest_to_pending() {
    let mut s = State::new();
    for (id, t) in [("a", 1000), ("b", 1001), ("c", 1002), ("d", 1003)] {
        add_at(&mut s, notif(id, 1), t);
    }
    assert_eq!(s.popups.len(), 4);
    assert_eq!(s.pending.len(), 0);

    add_at(&mut s, notif("e", 1), 1004);
    assert_eq!(s.popups.len(), 4);
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.pending[0].id, id("a")); // oldest evicted
    assert_eq!(ids(&s.popups), "b,c,d,e");
}

#[test]
fn add_caps_popups_at_four_groups_not_four_entries() {
    // MAX_POPUPS caps groups, so repeats of one notification cost no
    // unrelated toast its slot.
    let mut s = State::new();
    for (id, sum, t) in [
        ("a", "A", 1000),
        ("b", "B", 1001),
        ("c", "C", 1002),
        ("d", "D", 1003),
        ("d2", "D", 1004),
        ("d3", "D", 1005),
        ("d4", "D", 1006),
    ] {
        add_at(&mut s, summary(notif(id, 1), sum), t);
    }
    assert_eq!(s.popups.len(), 7);
    assert_eq!(group_entries(&s.popups).len(), 4);
    assert_eq!(s.pending.len(), 0);
}

#[test]
fn add_overflow_evicts_the_whole_oldest_group_to_pending() {
    let mut s = State::new();
    for (id, sum, t) in [
        ("a1", "A", 1000),
        ("a2", "A", 1001),
        ("b", "B", 1002),
        ("c", "C", 1003),
        ("d", "D", 1004),
    ] {
        add_at(&mut s, summary(notif(id, 1), sum), t);
    }
    assert_eq!(s.pending.len(), 0);

    add_at(&mut s, summary(notif("e", 1), "E"), 1005);
    assert_eq!(ids(&s.pending), "a1,a2");
    assert_eq!(ids(&s.popups), "b,c,d,e");
    assert_eq!(group_entries(&s.popups).len(), 4);
}

#[test]
fn expire_moves_timed_out_popups_to_pending_unseen() {
    let mut s = State::new();
    add_timeout(&mut s, notif("a", 1), 1000, 500); // expires 1500
    add_timeout(&mut s, notif("b", 1), 1000, 5000); // expires 6000
    s.expire(2000);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.popups[0].id, id("b"));
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.pending[0].id, id("a"));
    assert_eq!(s.pending[0].seen_at, None);
}

#[test]
fn expire_never_times_out_sticky_critical_popups() {
    let mut s = State::new();
    add_at(&mut s, from_notify_send(notif("a", 2)), 1000);
    s.expire(999_999_999);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.pending.len(), 0);
}

#[test]
fn expire_shrinks_a_group_member_by_member() {
    // Members keep their own clocks precisely because grouping is derived at
    // render time and never stored: a group of three shrinks to two, and the
    // expired member lands in pending on its own.
    let mut s = State::new();
    add_timeout(&mut s, summary(notif("a", 1), "Ping"), 1000, 500);
    add_timeout(&mut s, summary(notif("b", 1), "Ping"), 1001, 5000);
    add_timeout(&mut s, summary(notif("c", 1), "Ping"), 1002, 9000);
    assert_eq!(group_entries(&s.popups).len(), 1);
    assert_eq!(group_entries(&s.popups)[0].count, 3);

    s.expire(2000);
    assert_eq!(ids(&s.pending), "a");
    let groups = group_entries(&s.popups);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].count, 2);
    assert_eq!(groups[0].id, id("c"));
}

#[test]
fn update_patches_popup_content_in_place_without_moving_tier() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    s.update(id("a"), &patch_summary("Song B"), 5000);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.pending.len(), 0);
    assert_eq!(s.popups[0].summary, "Song B");
    assert_eq!(s.popups[0].arrived_at, 1000); // unchanged by a replace
}

#[test]
fn update_refreshes_expiry_for_still_popped_up_entry() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000); // expires 7000
    s.update(id("a"), &patch_summary("Song B"), 6500); // past the original clock
    assert_eq!(s.popups[0].expires_at, Some(12500)); // 6500 + 6000ms default, not expired
}

#[test]
fn update_keeps_sticky_expiry_for_critical_popup() {
    let mut s = State::new();
    add_at(&mut s, from_notify_send(notif("a", 2)), 1000);
    s.update(id("a"), &patch_summary("Song B"), 5000);
    assert_eq!(s.popups[0].expires_at, Some(0));
}

#[test]
fn update_patches_pending_entry_without_touching_expiry() {
    let mut s = dnd();
    add_at(&mut s, notif("a", 1), 1000);
    s.update(id("a"), &patch_summary("Song B"), 5000);
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.pending[0].summary, "Song B");
    assert_eq!(s.pending[0].expires_at, None);
}

#[test]
fn update_unknown_id_is_noop() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    let before = s.clone();
    s.update(id("missing"), &patch_summary("Song B"), 5000);
    assert_eq!(s, before);
}

#[test]
fn dismiss_popup_moves_to_past_marked_seen() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    s.dismiss_popup(id("a"), 5000);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.past.len(), 1);
    assert_eq!(s.past[0].id, id("a"));
    assert_eq!(s.past[0].seen_at, Some(5000));
}

#[test]
fn mark_all_seen_drains_pending_to_past() {
    let mut s = dnd();
    add_at(&mut s, notif("a", 1), 1000);
    add_at(&mut s, notif("b", 1), 1001);
    s.mark_all_seen(9000);
    assert_eq!(s.pending.len(), 0);
    assert_eq!(s.past.len(), 2);
    assert_eq!(s.past[0].seen_at, Some(9000));
    assert_eq!(s.past[1].seen_at, Some(9000));
}

#[test]
fn prune_past_drops_entries_older_than_15_minutes() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 0);
    s.dismiss_popup(id("a"), 1000); // seenAt 1000
    add_at(&mut s, notif("b", 1), 0);
    s.dismiss_popup(id("b"), 900_000); // seenAt exactly 15min after "now" below

    let fifteen_min = 15 * 60 * 1000;
    let now = 1000 + fifteen_min + 1;
    s.prune_past(now);
    let kept = ids(&s.past);
    assert!(!kept.contains('a'));
    assert!(kept.contains('b'));
}

#[test]
fn invoke_target_returns_most_recently_arrived_across_popups_and_pending() {
    let mut s = dnd();
    add_at(&mut s, notif("old", 1), 1000); // pending (dnd on)
    s.set_dnd(false);
    add_at(&mut s, notif("newer", 1), 2000); // popups
    assert_eq!(invoke_target(&s).unwrap().id, id("newer"));
}

#[test]
fn invoke_target_null_when_nothing_pending_or_popup() {
    assert!(invoke_target(&State::new()).is_none());
}

#[test]
fn dismiss_one_removes_from_any_tier() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    s.dismiss_popup(id("a"), 2000); // now in past
    s.dismiss_one(id("a"));
    assert_eq!(s.past.len(), 0);
}

#[test]
fn dismiss_many_drops_members_from_any_tier() {
    let mut s = dnd();
    add_at(&mut s, notif("p", 1), 1000); // pending
    s.set_dnd(false);
    add_at(&mut s, notif("q", 1), 1001); // popup
    add_at(&mut s, notif("r", 1), 1002);
    s.dismiss_popup(id("r"), 2000); // past
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.past.len(), 1);

    s.dismiss_many(&[id("p"), id("q"), id("r")]);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 0);
    assert_eq!(s.past.len(), 0);
    assert_eq!(s.next_expiry, None);
}

#[test]
fn dismiss_many_unknown_ids_returns_identity() {
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    let before = s.clone();
    s.dismiss_many(&[id("nope")]);
    assert_eq!(s, before);
}

#[test]
fn dismiss_all_clears_popups_only() {
    let mut s = dnd();
    add_at(&mut s, notif("a", 1), 1000); // pending
    s.set_dnd(false);
    add_at(&mut s, notif("b", 1), 1001); // popup
    s.dismiss_all();
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 1);
}

#[test]
fn clear_pending_empties_pending_only() {
    let mut s = dnd();
    add_at(&mut s, notif("a", 1), 1000);
    s.clear_pending();
    assert_eq!(s.pending.len(), 0);
}

#[test]
fn set_dnd_toggles_flag() {
    let mut s = dnd();
    assert!(s.dnd);
    s.set_dnd(false);
    assert!(!s.dnd);
}

#[test]
fn replace_by_id_never_creates_a_second_popup_entry() {
    // The freedesktop replaces_id contract: the service resyncs an existing
    // id via update() on every property change instead of calling add()
    // again, so the reducer must never grow a second entry for the same id.
    let mut s = State::new();
    add_at(&mut s, notif("a", 1), 1000);
    s.update(id("a"), &patch_summary("Song B"), 2000);
    s.update(id("a"), &patch_summary("Song C"), 3000);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.popups[0].summary, "Song C");
}

// group_key / group_entries

fn keyed(app_name: &str, summary: &str) -> Entry {
    Entry { app_name: app_name.into(), summary: summary.into(), ..Entry::default() }
}

#[test]
fn group_key_ignores_app_name_case_and_surrounding_whitespace() {
    assert_eq!(group_key(&keyed(" Slack ", "X")), group_key(&keyed("slack", "X")));
}

#[test]
fn group_key_ignores_body() {
    // Pins what counts as identical: appName + summary, never body.
    let mut a = keyed("Slack", "kyan");
    a.body = "hey".into();
    let mut b = keyed("Slack", "kyan");
    b.body = "you there?".into();
    assert_eq!(group_key(&a), group_key(&b));
}

#[test]
fn group_key_differs_on_summary() {
    assert_ne!(group_key(&keyed("Slack", "kyan")), group_key(&keyed("Slack", "someone else")));
}

#[test]
fn group_key_separator_prevents_field_boundary_collisions() {
    assert_ne!(group_key(&keyed("ab", "c")), group_key(&keyed("a", "bc")));
}

#[test]
fn group_entries_collapses_repeats_and_counts_them() {
    let mut s = State::new();
    add_at(&mut s, summary(notif("a", 1), "Ping"), 1000);
    add_at(&mut s, summary(notif("b", 1), "Ping"), 1001);
    add_at(&mut s, summary(notif("c", 1), "Ping"), 1002);
    add_at(&mut s, summary(notif("d", 1), "Other"), 1003);
    let groups = group_entries(&s.popups);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].summary, "Ping");
    assert_eq!(groups[0].count, 3);
    assert_eq!(groups[1].summary, "Other");
    assert_eq!(groups[1].count, 1);
}

#[test]
fn group_entries_representative_is_the_newest_member() {
    let mut s = State::new();
    let mut first = summary(notif("a", 1), "Ping");
    first.body = "first".into();
    let mut second = summary(notif("b", 1), "Ping");
    second.body = "second".into();
    add_timeout(&mut s, first, 1000, 500);
    add_timeout(&mut s, second, 2000, 500);
    let g = &group_entries(&s.popups)[0];
    assert_eq!(g.id, id("b"));
    assert_eq!(g.body, "second");
    assert_eq!(g.arrived_at, 2000);
    assert_eq!(g.expires_at, Some(2500));
}

#[test]
fn group_entries_member_ids_are_oldest_first() {
    let mut s = State::new();
    add_at(&mut s, summary(notif("a", 1), "Ping"), 1000);
    add_at(&mut s, summary(notif("b", 1), "Ping"), 1001);
    add_at(&mut s, summary(notif("c", 1), "Ping"), 1002);
    let g = &group_entries(&s.popups)[0];
    assert_eq!(g.count, 3);
    assert_eq!(g.member_ids.iter().map(|m| name(*m)).collect::<Vec<_>>().join(","), "a,b,c");

    // The center feeds `past` in newest-first order; the member ids and the
    // representative must not flip with it.
    let reversed: Vec<Entry> = s.popups.iter().rev().cloned().collect();
    let reversed = &group_entries(&reversed)[0];
    assert_eq!(reversed.member_ids.iter().map(|m| name(*m)).collect::<Vec<_>>().join(","), "a,b,c");
    assert_eq!(reversed.id, id("c"));
}

#[test]
fn group_entries_orders_groups_by_newest_member() {
    let mut s = State::new();
    add_at(&mut s, summary(notif("a", 1), "Ping"), 1000);
    add_at(&mut s, summary(notif("b", 1), "Other"), 1001);
    let order = |s: &State| {
        group_entries(&s.popups)
            .iter()
            .map(|g| g.summary.clone())
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(order(&s), "Ping,Other");

    // A repeat moves the whole group to the newest slot rather than adding a
    // row below.
    add_at(&mut s, summary(notif("c", 1), "Ping"), 1002);
    assert_eq!(order(&s), "Other,Ping");
    assert_eq!(group_entries(&s.popups).len(), 2);
}

#[test]
fn group_entries_of_empty_list_is_empty() {
    assert_eq!(group_entries(&[]).len(), 0);
}

#[test]
fn purity_group_entries_does_not_mutate_input() {
    let mut s = State::new();
    add_at(&mut s, summary(notif("a", 1), "Ping"), 1000);
    add_at(&mut s, summary(notif("b", 1), "Ping"), 1001);
    let before = s.clone();
    let _ = group_entries(&s.popups);
    assert_eq!(s, before);
}

// is_chromium_derived / sanitize_body / styled_body

#[test]
fn is_chromium_derived_matches_known_browsers_by_app_name() {
    assert!(is_chromium_derived("Google Chrome", ""));
    assert!(is_chromium_derived("Brave Browser", ""));
    assert!(is_chromium_derived("Vivaldi", ""));
    // Edge's Linux sender reports its binary name, not the title-case brand
    // string, and the marker list matches that literally.
    assert!(is_chromium_derived("microsoft-edge", ""));
    assert!(is_chromium_derived("Opera", ""));
}

#[test]
fn is_chromium_derived_matches_via_app_icon_too() {
    assert!(is_chromium_derived("Unknown", "google-chrome"));
}

#[test]
fn is_chromium_derived_false_for_unrelated_sender() {
    assert!(!is_chromium_derived("Slack", "slack"));
}

#[test]
fn sanitize_body_strips_img_tag_regardless_of_sender() {
    assert_eq!(sanitize_body("<img src=\"x.png\">Hello", "Slack", ""), "Hello");
}

#[test]
fn sanitize_body_leaves_non_chromium_url_prefix_untouched() {
    let body = "https://example.com/x Something happened";
    assert_eq!(sanitize_body(body, "Slack", ""), body);
}

#[test]
fn sanitize_body_strips_chromium_link_prefix_github_fixture() {
    // The exact "GH notifs are ugly" shape: Chrome glues a URL-as-link line
    // to the front of the body.
    let body = "<a href=\"https://github.com/notifications\">github.com</a> \
                New comment on issue #42: \"Fix the thing\"";
    assert_eq!(
        sanitize_body(body, "Google Chrome", ""),
        "New comment on issue #42: \"Fix the thing\""
    );
}

#[test]
fn sanitize_body_strips_chromium_bare_url_prefix() {
    let body = "github.com/owner/repo/pull/7 Review requested";
    assert_eq!(sanitize_body(body, "Google Chrome", ""), "Review requested");
}

#[test]
fn sanitize_body_chromium_sender_still_strips_img() {
    assert_eq!(
        sanitize_body("<img src=\"x.png\">Plain text", "Brave Browser", ""),
        "Plain text"
    );
}

// The server never advertises body-markup support, so a bare `<`/`&`/`>` in a
// sender's plain text is incidental, not markup: styled_body must escape it
// rather than hand it to a renderer that drops everything after an
// unterminated tag.

// button_actions

fn action(key: &str, label: &str) -> Action {
    Action { key: key.into(), label: label.into() }
}

fn with_actions(actions: Vec<Action>) -> Entry {
    Entry { actions, ..Entry::default() }
}

#[test]
fn button_actions_drops_the_default_activation_hint() {
    // Ghostty's shape: a `default` action with no label at all, which used
    // to reach the card as an empty pill.
    assert_eq!(button_actions(&with_actions(vec![action("default", "")])).len(), 0);
}

#[test]
fn button_actions_drops_default_even_when_it_carries_a_label() {
    assert_eq!(button_actions(&with_actions(vec![action("default", "Open")])).len(), 0);
}

#[test]
fn button_actions_drops_a_labelless_ordinary_action() {
    assert_eq!(button_actions(&with_actions(vec![action("reply", "  ")])).len(), 0);
}

#[test]
fn button_actions_keeps_labelled_actions_in_order() {
    let acts = button_actions(&with_actions(vec![
        action("default", ""),
        action("reply", "Reply"),
        action("mute", "Mute"),
    ]));
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0].key, "reply");
    assert_eq!(acts[1].key, "mute");
}

#[test]
fn button_actions_tolerates_a_missing_actions_list() {
    assert_eq!(button_actions(&Entry::default()).len(), 0);
}

// position_spec

#[test]
fn position_spec_top_right() {
    let spec = position_spec(Some("top-right"));
    assert_eq!(spec.name.as_str(), "top-right");
    assert!(spec.top);
    assert!(!spec.bottom);
    assert!(!spec.left);
    assert!(spec.right);
}

#[test]
fn position_spec_top_left() {
    let spec = position_spec(Some("top-left"));
    assert_eq!(spec.name.as_str(), "top-left");
    assert!(spec.top);
    assert!(!spec.bottom);
    assert!(spec.left);
    assert!(!spec.right);
}

#[test]
fn position_spec_bottom_right_is_the_default() {
    let spec = position_spec(Some("bottom-right"));
    assert_eq!(spec.name.as_str(), "bottom-right");
    assert!(!spec.top);
    assert!(spec.bottom);
    assert!(!spec.left);
    assert!(spec.right);
    assert_eq!(DEFAULT_POSITION.as_str(), "bottom-right");
}

#[test]
fn position_spec_bottom_left() {
    let spec = position_spec(Some("bottom-left"));
    assert_eq!(spec.name.as_str(), "bottom-left");
    assert!(!spec.top);
    assert!(spec.bottom);
    assert!(spec.left);
    assert!(!spec.right);
}

#[test]
fn position_spec_falls_back_to_default_on_garbage() {
    assert_eq!(position_spec(Some("diagonal")).name, DEFAULT_POSITION);
    assert_eq!(position_spec(Some("")).name, DEFAULT_POSITION);
    // A missing or non-string setting (`undefined`, `null`, a number) reaches
    // here as `None`.
    assert_eq!(position_spec(None).name, DEFAULT_POSITION);
}

// stack_order

fn group(name: &str, arrived_at: i64, urgency: u8) -> Group {
    Group {
        entry: Entry {
            id: id(name),
            arrived_at,
            urgency: Urgency::try_from(urgency).unwrap(),
            ..Entry::default()
        },
        count: 1,
        member_ids: vec![id(name)],
    }
}

fn group_ids(groups: &[Group]) -> String {
    groups.iter().map(|g| name(g.id)).collect::<Vec<_>>().join(",")
}

#[test]
fn stack_order_newest_first_with_no_critical() {
    let order = stack_order(&[group("a", 1000, 1), group("b", 3000, 1), group("c", 2000, 1)]);
    assert_eq!(group_ids(&order), "b,c,a");
}

#[test]
fn stack_order_critical_wins_front_over_newer_normal() {
    let order = stack_order(&[group("old-crit", 1000, 2), group("newer", 3000, 1)]);
    assert_eq!(order[0].id, id("old-crit"));
    assert_eq!(order[1].id, id("newer"));
}

#[test]
fn stack_order_critical_already_newest_is_a_noop() {
    let order = stack_order(&[group("normal", 1000, 1), group("crit", 3000, 2)]);
    assert_eq!(group_ids(&order), "crit,normal");
}

#[test]
fn stack_order_two_criticals_newest_critical_wins_front() {
    let order = stack_order(&[
        group("crit-old", 1000, 2),
        group("normal", 1500, 1),
        group("crit-new", 2000, 2),
    ]);
    assert_eq!(order[0].id, id("crit-new"));
    // The rest stays newest-first among what's left.
    assert_eq!(group_ids(&order[1..]), "normal,crit-old");
}

#[test]
fn stack_order_empty_is_empty() {
    assert_eq!(stack_order::<Group>(&[]).len(), 0);
}

#[test]
fn stack_order_single_entry() {
    let order = stack_order(&[group("only", 1000, 2)]);
    assert_eq!(order.len(), 1);
    assert_eq!(order[0].id, id("only"));
}

#[test]
fn purity_stack_order_does_not_mutate_input() {
    let input = [group("a", 1000, 1), group("b", 3000, 2)];
    let before = input.clone();
    let _ = stack_order(&input);
    assert_eq!(input, before);
}

// notifications.sound: elementary's map, and urgency outranking the category
// in it: a critical notification is a warning whatever it is about.

const LOW: Urgency = Urgency::Low;
const NORMAL: Urgency = Urgency::Normal;
const CRITICAL: Urgency = Urgency::Critical;

#[test]
fn a_plain_notification_takes_the_information_sound() {
    assert_eq!(sound_name(NORMAL, Some("")), "dialog-information");
    assert_eq!(sound_name(LOW, Some("")), "dialog-information");
}

#[test]
fn an_instant_message_has_a_sound_of_its_own() {
    assert_eq!(sound_name(NORMAL, Some("im.received")), "message-new-instant");
    assert_eq!(sound_name(LOW, Some("im.received")), "message-new-instant");
}

#[test]
fn critical_is_a_warning_whatever_its_category_says() {
    assert_eq!(sound_name(CRITICAL, Some("")), "dialog-warning");
    assert_eq!(sound_name(CRITICAL, Some("im.received")), "dialog-warning");
}

// A sender that sent no category hint at all, which is most of them.
#[test]
fn a_missing_category_reads_as_no_category() {
    assert_eq!(sound_name(NORMAL, None), "dialog-information");
}

// Every category but the one elementary names is the plain sound: the map is
// deliberately three answers, not a table to grow by guessing.
#[test]
fn every_other_category_is_the_information_sound() {
    assert_eq!(sound_name(NORMAL, Some("email.arrived")), "dialog-information");
    assert_eq!(sound_name(NORMAL, Some("device.added")), "dialog-information");
}

#[test]
fn badge_counts_unseen_then_falls_back_to_history() {
    let mut s = State::new();
    assert_eq!((s.unseen(), s.total(), s.badge()), (0, 0, 0));
    add_at(&mut s, notif("a", 1), 1000);
    add_at(&mut s, notif("b", 1), 1001);
    s.dismiss_popup(id("a"), 2000);
    assert_eq!((s.unseen(), s.total(), s.badge()), (1, 2, 1));
    s.dismiss_popup(id("b"), 2000);
    assert_eq!((s.unseen(), s.total(), s.badge()), (0, 2, 2));
}
