use chrono::DateTime;
use fs_info::usage::*;
use serde_json::json;

fn ms(iso: &str) -> f64 {
    DateTime::parse_from_rfc3339(iso).unwrap().timestamp_millis() as f64
}

// parse_credentials

#[test]
fn parse_credentials_extracts_oauth_fields() {
    let r = parse_credentials(
        &json!({ "claudeAiOauth": {
            "accessToken": "tok123",
            "refreshToken": "ref456",
            "expiresAt": 1700000000000_i64,
            "subscriptionType": "pro",
            "rateLimitTier": "max_20x"
        } })
        .to_string(),
    )
    .unwrap();
    assert_eq!(r.access_token, "tok123");
    assert!(r.has_refresh_token);
    assert_eq!(r.expires_at_ms, 1700000000000.0);
    assert_eq!(r.subscription_type, "pro");
    assert_eq!(r.rate_limit_tier, "max_20x");
}

// The refresh token is what separates "logged out" from "logged in, access
// token needs a claude run to refresh": its value never leaves the parser,
// only its presence.
#[test]
fn parse_credentials_never_returns_the_refresh_token_itself() {
    let r = parse_credentials(
        &json!({ "claudeAiOauth": { "accessToken": "tok", "refreshToken": "ref456" } }).to_string(),
    )
    .unwrap();
    assert!(r.has_refresh_token);
    assert!(!format!("{r:?}").contains("ref456"));
}

#[test]
fn parse_credentials_absent_refresh_token() {
    let r = parse_credentials(&json!({ "claudeAiOauth": { "accessToken": "tok" } }).to_string()).unwrap();
    assert!(!r.has_refresh_token);
}

#[test]
fn parse_credentials_empty_refresh_token_counts_as_absent() {
    let r = parse_credentials(
        &json!({ "claudeAiOauth": { "accessToken": "tok", "refreshToken": "" } }).to_string(),
    )
    .unwrap();
    assert!(!r.has_refresh_token);
}

#[test]
fn parse_credentials_malformed_json() {
    let e = parse_credentials("not json{{{").unwrap_err();
    assert_eq!(e.as_str(), "malformed_json");
}

#[test]
fn parse_credentials_missing_oauth_block() {
    let e = parse_credentials(&json!({ "other": true }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_credentials_empty_access_token() {
    let e = parse_credentials(&json!({ "claudeAiOauth": { "accessToken": "" } }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_credentials_missing_expiry_defaults_to_zero() {
    let r = parse_credentials(&json!({ "claudeAiOauth": { "accessToken": "tok" } }).to_string()).unwrap();
    assert_eq!(r.expires_at_ms, 0.0);
}

// credentials_expired

#[test]
fn credentials_expired_true_past_deadline() {
    assert!(credentials_expired(1000.0, 2000.0));
}

#[test]
fn credentials_expired_false_before_deadline() {
    assert!(!credentials_expired(3000.0, 2000.0));
}

#[test]
fn credentials_expired_false_when_no_expiry_info() {
    assert!(!credentials_expired(0.0, 2000.0));
}

// parse_usage

#[test]
fn parse_usage_extracts_five_hour_and_seven_day_rows() {
    let rows = parse_usage(
        &json!({
            "five_hour": { "utilization": 42.0, "resets_at": "2026-08-02T10:00:00Z" },
            "seven_day": { "utilization": 10.0, "resets_at": "2026-08-05T00:00:00Z" }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].label, "5-hour");
    assert_eq!(rows[0].used.as_fraction(), 0.42);
    assert_eq!(rows[0].resets_at, "2026-08-02T10:00:00Z");
    assert_eq!(rows[1].label, "Weekly");
    assert_eq!(rows[1].used.as_fraction(), 0.1);
}

#[test]
fn parse_usage_enumerates_dynamic_buckets_in_stable_order() {
    let rows = parse_usage(
        &json!({
            "seven_day_sonnet": { "utilization": 8.0, "resets_at": "2026-08-05T00:00:00Z" },
            "experimental_window": { "utilization": 3.0, "resets_at": "2026-08-05T00:00:00Z" },
            "five_hour": { "utilization": 42.0, "resets_at": "2026-08-02T10:00:00Z" },
            "seven_day_opus": { "utilization": 15.0, "resets_at": "2026-08-05T00:00:00Z" },
            "seven_day": { "utilization": 10.0, "resets_at": "2026-08-05T00:00:00Z" }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].label, "5-hour");
    assert_eq!(rows[1].label, "Weekly");
    assert_eq!(rows[2].label, "Experimental window");
    assert_eq!(rows[3].label, "Weekly opus");
    assert_eq!(rows[4].label, "Weekly sonnet");
    assert_eq!(rows[3].used.as_fraction(), 0.15);
    assert_eq!(rows[4].used.as_fraction(), 0.08);
}

#[test]
fn parse_usage_skips_null_utilization_bucket_without_dropping_others() {
    let rows = parse_usage(
        &json!({
            "five_hour": { "utilization": 42.0, "resets_at": "2026-08-02T10:00:00Z" },
            "seven_day_opus": { "utilization": null, "resets_at": "2026-08-05T00:00:00Z" }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "5-hour");
}

#[test]
fn parse_usage_all_buckets_null_utilization_is_missing_fields() {
    let e = parse_usage(
        &json!({ "seven_day_opus": { "utilization": null, "resets_at": "2026-08-05T00:00:00Z" } })
            .to_string(),
    )
    .unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_usage_treats_fraction_scale_when_nothing_is_percent() {
    let rows = parse_usage(
        &json!({ "five_hour": { "utilization": 0.37 }, "seven_day": { "utilization": 0.05 } }).to_string(),
    )
    .unwrap();
    assert_eq!(rows[0].used.as_fraction(), 0.37);
    assert_eq!(rows[1].used.as_fraction(), 0.05);
}

#[test]
fn parse_usage_missing_both_buckets() {
    let e = parse_usage(&json!({ "other": true }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_usage_malformed_json() {
    assert_eq!(parse_usage("not json{{{").unwrap_err().as_str(), "malformed_json");
}

#[test]
fn parse_usage_negative_utilization_dropped() {
    let e = parse_usage(&json!({ "five_hour": { "utilization": -1 } }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

// tier_label

#[test]
fn tier_label_max_tier_wins_over_subscription() {
    assert_eq!(tier_label("pro", "max_20x"), "Max 20x");
}

#[test]
fn tier_label_falls_back_to_capitalized_subscription() {
    assert_eq!(tier_label("pro", ""), "Pro");
}

#[test]
fn tier_label_empty_when_both_absent() {
    assert_eq!(tier_label("", ""), "");
}

// format_reset

#[test]
fn format_reset_hours_and_minutes() {
    let now = ms("2026-08-02T10:00:00Z");
    assert_eq!(format_reset(now, "2026-08-02T12:14:00Z"), "Resets 2H 14M");
}

#[test]
fn format_reset_minutes_only_under_an_hour() {
    let now = ms("2026-08-02T10:00:00Z");
    assert_eq!(format_reset(now, "2026-08-02T10:45:00Z"), "Resets 45M");
}

#[test]
fn format_reset_days_and_hours_over_a_day() {
    let now = ms("2026-08-02T10:00:00Z");
    assert_eq!(format_reset(now, "2026-08-04T13:00:00Z"), "Resets 2D 3H");
}

#[test]
fn format_reset_now_when_already_past() {
    let now = ms("2026-08-02T10:00:00Z");
    assert_eq!(format_reset(now, "2026-08-02T09:00:00Z"), "Resets now");
}

#[test]
fn format_reset_empty_for_no_timestamp() {
    assert_eq!(format_reset(ms("2026-08-02T10:00:00Z"), ""), "");
}

#[test]
fn format_reset_reads_the_endpoints_microsecond_offset_form() {
    let now = ms("2026-08-02T10:00:00Z");
    assert_eq!(format_reset(now, "2026-08-02T12:14:00.123456+00:00"), "Resets 2H 14M");
}

// refresh_state_for_exit / refresh_hint

#[test]
fn refresh_state_for_exit_separates_a_missing_cli_from_a_failure() {
    assert_eq!(refresh_state_for_exit(0), RefreshState::Ok);
    assert_eq!(refresh_state_for_exit(127), RefreshState::NoCli);
    assert_eq!(refresh_state_for_exit(1), RefreshState::Failed);
    assert_eq!(refresh_state_for_exit(2), RefreshState::Failed);
}

#[test]
fn refresh_hint_names_the_step_left_to_the_owner() {
    assert_eq!(refresh_hint(RefreshState::Running), "Refreshing");
    assert_eq!(refresh_hint(RefreshState::NoCli), "No Claude CLI");
    assert_eq!(refresh_hint(RefreshState::Failed), "Run claude auth login");
    assert_eq!(refresh_hint(RefreshState::Idle), "Run claude to refresh");
}

// A clean helper run that left the token stale is not a refresh problem, so
// it reads as the pre-refresh ask rather than as a success.
#[test]
fn refresh_hint_treats_a_clean_run_that_changed_nothing_as_idle() {
    assert_eq!(refresh_hint(RefreshState::Ok), "Run claude to refresh");
}

// parse_codex_account

#[test]
fn parse_codex_account_extracts_plan_type() {
    let r = parse_codex_account(&json!({ "id": 2, "result": { "account": { "planType": "team" } } }).to_string());
    assert_eq!(r.unwrap(), "team");
}

#[test]
fn parse_codex_account_falls_back_to_type_field() {
    let r = parse_codex_account(&json!({ "id": 2, "result": { "account": { "type": "individual" } } }).to_string());
    assert_eq!(r.unwrap(), "individual");
}

#[test]
fn parse_codex_account_missing_account() {
    let e = parse_codex_account(&json!({ "id": 2, "result": {} }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_codex_account_malformed_json() {
    assert_eq!(parse_codex_account("not json{{{").unwrap_err().as_str(), "malformed_json");
}

// parse_codex_rate_limits

#[test]
fn parse_codex_rate_limits_extracts_primary_and_secondary() {
    let r = parse_codex_rate_limits(
        &json!({
            "id": 3,
            "result": {
                "rateLimits": {
                    "planType": "team",
                    "primary": { "usedPercent": 12.5, "windowDurationMins": 300, "resetsAt": 1785700800 },
                    "secondary": { "usedPercent": 40, "windowDurationMins": 10080, "resetsAt": 1785700800 }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(r.plan_type, "team");
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0].label, "5h window");
    assert_eq!(r.rows[0].used.as_fraction(), 0.125);
    assert_eq!(r.rows[0].resets_at, "2026-08-02T20:00:00.000Z");
    assert_eq!(r.rows[1].label, "Weekly");
    assert_eq!(r.rows[1].used.as_fraction(), 0.4);
}

#[test]
fn parse_codex_rate_limits_labels_non_hour_windows_in_minutes() {
    let r = parse_codex_rate_limits(
        &json!({
            "id": 3,
            "result": { "rateLimits": { "primary": { "usedPercent": 5, "windowDurationMins": 90 } } }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(r.rows[0].label, "90m window");
}

#[test]
fn parse_codex_rate_limits_missing_limits() {
    let e = parse_codex_rate_limits(&json!({ "id": 3, "result": {} }).to_string()).unwrap_err();
    assert_eq!(e.as_str(), "missing_fields");
}

#[test]
fn parse_codex_rate_limits_only_secondary_present() {
    let r = parse_codex_rate_limits(
        &json!({
            "id": 3,
            "result": { "rateLimits": { "secondary": { "usedPercent": 8, "windowDurationMins": 10080 } } }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.rows[0].label, "Weekly");
}
