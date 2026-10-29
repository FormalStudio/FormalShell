use fs_info::agents::*;
use serde_json::json;

#[test]
fn is_app_by_app_id_or_class() {
    assert!(is_app("com.t3tools.T3Code", ""));
    assert!(is_app("", "com.t3tools.T3Code"));
    assert!(!is_app("foot", "foot"));
    assert!(!is_app("", ""));
}

#[test]
fn aggregate_worst_wins() {
    let t = |s: &str| json!({"status": s});
    assert_eq!(aggregate(&[]), Badge::None);
    assert_eq!(aggregate(&[t("done")]), Badge::Done);
    assert_eq!(aggregate(&[t("done"), t("working")]), Badge::Working);
    assert_eq!(aggregate(&[t("working"), t("blocked"), t("done")]), Badge::Blocked);
    assert_eq!(aggregate(&[json!({}), t("bogus")]), Badge::None);
}

#[test]
fn parse_line_reads_threads() {
    let line = r#"{"environment":"e","threads":[{"id":"a","title":"x","project":"p","status":"working","seq":"1"}]}"#;
    assert_eq!(parse_line(Some(line)), Some(Badge::Working));
    assert_eq!(parse_line(Some(r#"{"environment":"e","threads":[]}"#)), Some(Badge::None));
}

#[test]
fn parse_line_rejects_other_shapes() {
    assert_eq!(parse_line(None), None);
    assert_eq!(parse_line(Some("")), None);
    assert_eq!(parse_line(Some("null")), None);
    assert_eq!(parse_line(Some("{not json")), None);
    assert_eq!(parse_line(Some(r#"{"environment":"e"}"#)), None);
}

#[test]
fn badge_order_and_strings() {
    assert!(Badge::Blocked > Badge::Working && Badge::Working > Badge::Done && Badge::Done > Badge::None);
    for b in [Badge::None, Badge::Done, Badge::Working, Badge::Blocked] {
        assert_eq!(Badge::parse(b.as_str()), b);
    }
}

#[test]
fn poll_command_is_one_ssh_per_host() {
    let argv = poll_command("macbook");
    assert_eq!(argv[0], "ssh");
    assert!(argv.contains(&"-n".to_owned()));
    assert!(argv.contains(&"BatchMode=yes".to_owned()));
    assert_eq!(&argv[argv.len() - 2..], ["macbook", ".local/bin/t3-agents"]);
}
