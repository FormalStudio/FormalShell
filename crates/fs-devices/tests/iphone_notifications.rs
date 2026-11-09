//! The iPhone mirror against the notification model: the replace path, the
//! quiet and DND tiers and the source-keyed grouping, with fs-info's reducer
//! standing where the notification service runs `Model.add`.

use fs_devices::iphone::{DedupeRule, Event, Notification, dedupe_rules, default_dedupe, parse_event, superseded, to_notification};
use fs_info::notifications as nm;

const LINE_SMS: &str = r#"{"type": "notification", "id": 4012, "appId": "com.apple.MobileSMS", "appName": "Messages", "title": "Alex", "subtitle": "", "body": "Still on for Friday?", "deviceName": "Kyan's iPhone", "deviceHandle": "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6", "positiveAction": null, "negativeAction": "Clear", "category": 4, "categoryCount": 2, "silent": false, "important": false, "preexisting": false, "session": 77, "ts": 1790000000.0}"#;
const LINE_CALL: &str = r#"{"type": "notification", "id": 4013, "appId": "com.apple.mobilephone", "appName": "Phone", "title": "Mum", "subtitle": "", "body": "mobile", "deviceName": "Kyan's iPhone", "deviceHandle": "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6", "positiveAction": "Answer", "negativeAction": "Decline", "category": 1, "categoryCount": 1, "silent": false, "important": true, "preexisting": false, "session": 77, "ts": 1790000001.0}"#;

fn rules() -> Vec<DedupeRule> {
    dedupe_rules(&default_dedupe())
}

fn parse_notification(line: &str) -> Notification {
    match parse_event(line) {
        Some(Event::Notification(n)) => n,
        other => panic!("not a notification: {other:?}"),
    }
}

fn phone() -> Notification {
    parse_notification(LINE_SMS)
}

/// A local desktop client's notification, as the server layer hands `add`.
fn local(id: &str, summary: &str, body: &str) -> nm::Notif {
    nm::Notif {
        id: id.into(),
        app_name: "Messages".into(),
        summary: summary.into(),
        body: body.into(),
        urgency: nm::Urgency::Normal,
        ..nm::Notif::default()
    }
}

fn all_entries(state: &nm::State) -> impl Iterator<Item = &nm::Entry> {
    state.popups.iter().chain(&state.pending).chain(&state.past)
}

fn add(state: &nm::State, notif: &nm::Notif, now: i64) -> nm::State {
    nm::add(state, notif, now, &nm::AddOpts::default())
}

fn add_quiet(state: &nm::State, notif: &nm::Notif, now: i64) -> nm::State {
    nm::add(
        state,
        notif,
        now,
        &nm::AddOpts {
            quiet: true,
            ..nm::AddOpts::default()
        },
    )
}

fn entry_of(notif: &nm::Notif, arrived_at: i64) -> nm::Entry {
    let s = add(&nm::State::new(), notif, arrived_at);
    s.popups[0].clone()
}

// The replace path end to end through the notification reducer: a phone card
// on screen, then the desktop client raising the same message, leaves one
// card, the local one.
#[test]
fn local_arrival_replaces_phone_card() {
    let now = 100000;
    let mut s = nm::State::new();
    s = add(&s, &to_notification(&phone()), now);
    s = add(&s, &local("9", "Alex", "Still on for Friday?"), now + 3000);
    assert_eq!(s.popups.len(), 2);
    let incoming = s.popups.iter().find(|p| p.id == "9").unwrap();
    let stale = superseded(&rules(), incoming, all_entries(&s), (now + 3000) as f64);
    assert_eq!(stale, vec!["iphone-77-4012".to_string()]);
    s = nm::dismiss_many(&s, &stale);
    assert_eq!(s.popups.len(), 1);
    assert_eq!(s.popups[0].id, "9");
    assert_eq!(s.pending.len() + s.past.len(), 0);
}

#[test]
fn local_arrival_replaces_quiet_phone_entry_in_history() {
    let now = 100000;
    let record = Notification {
        silent: true,
        ..phone()
    };
    let mut s = add_quiet(&nm::State::new(), &to_notification(&record), now);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 1);
    let incoming = entry_of(&local("9", "Alex", "Still on for Friday?"), now + 1000);
    let stale = superseded(&rules(), &incoming, all_entries(&s), (now + 1000) as f64);
    assert_eq!(stale.len(), 1);
    s = nm::dismiss_many(&s, &stale);
    assert_eq!(s.pending.len(), 0);
}

#[test]
fn quiet_add_goes_to_pending_without_dnd() {
    let s = add_quiet(&nm::State::new(), &to_notification(&phone()), 1000);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 1);
    assert_eq!(s.pending[0].source, "iphone");
    assert_eq!(s.pending[0].phone.as_ref().unwrap().id, 4012);
}

#[test]
fn phone_and_local_cards_never_group() {
    let a = entry_of(&to_notification(&phone()), 1000);
    let b = entry_of(&local("9", "Alex", "different"), 1000);
    assert_ne!(nm::group_key(&a), nm::group_key(&b));
    assert_eq!(nm::group_entries(&[a, b]).len(), 2);
}

#[test]
fn critical_phone_entry_still_waits_behind_dnd() {
    let call = parse_notification(LINE_CALL);
    let s = nm::set_dnd(&nm::State::new(), true);
    let s = add(&s, &to_notification(&call), 1000);
    assert_eq!(s.popups.len(), 0);
    assert_eq!(s.pending.len(), 1);
}
