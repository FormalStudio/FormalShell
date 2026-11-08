//! Which registered MPRIS source the shell speaks to, the loop-state naming
//! and cycle order, and the two value clamps. Rows are plain data built by the
//! caller from live player properties.

use serde_json::Value;

use fs_js::utf16_len;
use fs_types::Fraction;

pub(crate) const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// `None` fields mirror a property the caller left off the row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerRow {
    pub id: String,
    pub kind: Option<String>,
    /// `Some(false)` is an app with audio and no MPRIS: only ever picked by hand.
    pub auto: Option<bool>,
    pub identity: Option<String>,
    pub is_playing: bool,
    /// Registered with no track: a browser keeps its player on the bus after
    /// the tab that played closes.
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelledRow {
    pub id: String,
    pub kind: String,
    pub auto: bool,
    pub identity: Option<String>,
    pub label: String,
    pub is_playing: bool,
}

/// An explicit pick wins while its source is still there; otherwise the first
/// playing one, then the first carrying a track, then the first. Rows with
/// `auto == Some(false)` are never picked automatically.
pub fn pick_player_id(rows: &[PlayerRow], selected_id: &str) -> String {
    if rows.is_empty() {
        return String::new();
    }
    if !selected_id.is_empty() && rows.iter().any(|r| r.id == selected_id) {
        return selected_id.to_string();
    }
    let mut first = "";
    let mut first_track = "";
    for r in rows {
        if r.auto == Some(false) {
            continue;
        }
        if r.is_playing {
            return r.id.clone();
        }
        if first.is_empty() {
            first = &r.id;
        }
        if first_track.is_empty() && !r.empty {
            first_track = &r.id;
        }
    }
    if first_track.is_empty() { first } else { first_track }.to_string()
}

/// The bus name's first word and the identity, lowercased, each at least three
/// UTF-16 units long.
fn player_tokens(row: &PlayerRow) -> Vec<String> {
    let mut out = Vec::new();
    let bus = bus_suffix(&row.id)
        .split('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if utf16_len(&bus) >= 3 {
        out.push(bus);
    }
    let identity = row.identity.as_deref().unwrap_or("").to_lowercase();
    if utf16_len(&identity) >= 3 && !out.contains(&identity) {
        out.push(identity);
    }
    out
}

/// Which player row a playback stream belongs to, or "". `keys` are the
/// stream's application.name, application.process.binary, application.id and
/// node.name. A key and a token match when one holds the other. MPRIS carries
/// no pid, so a name is all there is to go on.
pub fn stream_owner(keys: &[Option<&str>], rows: &[PlayerRow]) -> String {
    let names: Vec<String> = keys
        .iter()
        .map(|k| k.unwrap_or("").to_lowercase())
        .filter(|k| utf16_len(k) >= 3)
        .collect();
    for row in rows {
        for token in player_tokens(row) {
            for name in &names {
                if name.contains(&token) || token.contains(name.as_str()) {
                    return row.id.clone();
                }
            }
        }
    }
    String::new()
}

/// The sink-input index `pactl move-sink-input` takes for a Pipewire node, out
/// of `pactl -f json list sink-inputs`, or -1.
pub fn sink_input_index(text: &str, node_id: impl std::fmt::Display) -> i64 {
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(text) else {
        return -1;
    };
    let want = node_id.to_string();
    for row in &rows {
        let Some(props) = row.get("properties") else {
            continue;
        };
        let object_id = match props.get("object.id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => "undefined".to_string(),
        };
        if object_id == want
            && let Some(Value::Number(index)) = row.get("index")
        {
            return index.as_f64().map(|f| f as i64).unwrap_or(-1);
        }
    }
    -1
}

/// What is left of a bus name once the well-known prefix is off it.
pub(crate) fn bus_suffix(id: &str) -> &str {
    if let Some(rest) = id.strip_prefix(BUS_PREFIX) {
        return rest;
    }
    match id.rfind('.') {
        Some(dot) => &id[dot + 1..],
        None => id,
    }
}

/// The identity a player publishes for itself, else its bus name.
pub(crate) fn player_label(row: &PlayerRow) -> String {
    match row.identity.as_deref() {
        Some(identity) if !identity.is_empty() => identity.to_string(),
        _ => bus_suffix(&row.id).to_string(),
    }
}

/// Rows whose label collides fall back to the bus name, which is unique by
/// construction; rows with a label of their own keep it.
pub fn with_labels(rows: &[PlayerRow]) -> Vec<LabelledRow> {
    let labels: Vec<String> = rows.iter().map(player_label).collect();
    rows.iter()
        .zip(&labels)
        .map(|(row, label)| {
            let collides = labels.iter().filter(|l| *l == label).count() > 1;
            LabelledRow {
                id: row.id.clone(),
                kind: row.kind.clone().unwrap_or_else(|| "mpris".to_string()),
                auto: row.auto != Some(false),
                identity: row.identity.clone(),
                label: if collides {
                    bus_suffix(&row.id).to_string()
                } else {
                    label.clone()
                },
                is_playing: row.is_playing,
            }
        })
        .collect()
}

/// Off, repeat the queue, repeat this track, off.
pub fn next_loop(name: &str) -> &'static str {
    match name {
        "none" => "playlist",
        "playlist" => "track",
        _ => "none",
    }
}

/// The share of the flat track a source's 0..1 volume fills: MPRIS Volume
/// above 1.0 is legal on the wire but the track never draws past full.
/// Non-finite is 0.
pub fn clamp_volume(v: f64) -> Fraction {
    if !v.is_finite() {
        return Fraction::from_fraction(0.0);
    }
    Fraction::from_fraction(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, playing: bool, identity: &str) -> PlayerRow {
        PlayerRow {
            id: id.into(),
            is_playing: playing,
            identity: Some(identity.into()),
            ..Default::default()
        }
    }

    fn bare(id: &str) -> PlayerRow {
        row(id, false, "")
    }

    #[test]
    fn pick_empty_for_no_players() {
        assert_eq!(pick_player_id(&[], ""), "");
        assert_eq!(pick_player_id(&[], "org.mpris.MediaPlayer2.mpv"), "");
    }

    #[test]
    fn pick_first_when_nothing_playing() {
        assert_eq!(pick_player_id(&[bare("a"), bare("b")], ""), "a");
    }

    #[test]
    fn pick_a_track_over_an_empty_player() {
        let empty = PlayerRow { empty: true, ..bare("a") };
        assert_eq!(pick_player_id(&[empty.clone(), bare("b")], ""), "b");
        assert_eq!(pick_player_id(&[empty.clone()], ""), "a");
        assert_eq!(pick_player_id(&[empty, bare("b")], "a"), "a");
    }

    #[test]
    fn pick_playing_over_first_registered() {
        assert_eq!(pick_player_id(&[bare("a"), row("b", true, "")], ""), "b");
    }

    #[test]
    fn pick_first_playing_when_several_play() {
        let rows = [bare("a"), row("b", true, ""), row("c", true, "")];
        assert_eq!(pick_player_id(&rows, ""), "b");
    }

    #[test]
    fn selection_wins_over_a_playing_player() {
        assert_eq!(pick_player_id(&[bare("a"), row("b", true, "")], "a"), "a");
    }

    #[test]
    fn selection_that_quit_falls_back() {
        assert_eq!(
            pick_player_id(&[bare("a"), row("b", true, "")], "gone"),
            "b"
        );
    }

    #[test]
    fn label_prefers_identity() {
        assert_eq!(
            player_label(&row(
                "org.mpris.MediaPlayer2.mpv",
                false,
                "mpv Media Player"
            )),
            "mpv Media Player"
        );
    }

    #[test]
    fn label_falls_back_to_bus_name_tail() {
        assert_eq!(player_label(&bare("org.mpris.MediaPlayer2.mpv")), "mpv");
    }

    #[test]
    fn label_of_a_dotless_bus_name_is_the_name() {
        assert_eq!(player_label(&bare("mpv")), "mpv");
    }

    #[test]
    fn labels_are_kept_when_they_are_already_distinct() {
        let rows = with_labels(&[
            row("org.mpris.MediaPlayer2.mpv", true, "mpv"),
            row(
                "org.mpris.MediaPlayer2.firefox.instance-7",
                false,
                "Mozilla Firefox",
            ),
        ]);
        assert_eq!(rows[0].label, "mpv");
        assert_eq!(rows[1].label, "Mozilla Firefox");
        assert!(rows[0].is_playing);
        assert_eq!(rows[1].id, "org.mpris.MediaPlayer2.firefox.instance-7");
    }

    #[test]
    fn colliding_labels_fall_back_to_the_bus_name() {
        let rows = with_labels(&[
            row("org.mpris.MediaPlayer2.mpv", false, "mpv"),
            row("org.mpris.MediaPlayer2.mpv.instance-abc", false, "mpv"),
        ]);
        assert_eq!(rows[0].label, "mpv");
        assert_eq!(rows[1].label, "mpv.instance-abc");
    }

    #[test]
    fn only_the_colliding_rows_lose_their_identity() {
        let rows = with_labels(&[
            row("org.mpris.MediaPlayer2.mpv", false, "mpv"),
            row("org.mpris.MediaPlayer2.mpv.instance-abc", false, "mpv"),
            row("org.mpris.MediaPlayer2.spotify", false, "Spotify"),
        ]);
        assert_eq!(rows[2].label, "Spotify");
    }

    #[test]
    fn labels_of_nothing_is_an_empty_list() {
        assert!(with_labels(&[]).is_empty());
    }

    #[test]
    fn loop_cycles_off_queue_track_off() {
        assert_eq!(next_loop("none"), "playlist");
        assert_eq!(next_loop("playlist"), "track");
        assert_eq!(next_loop("track"), "none");
    }

    #[test]
    fn loop_cycle_of_garbage_lands_on_none() {
        assert_eq!(next_loop("nonsense"), "none");
    }

    #[test]
    fn volume_clamped_to_the_flat_track() {
        assert_eq!(clamp_volume(0.5).as_fraction(), 0.5);
        assert_eq!(clamp_volume(-1.0).as_fraction(), 0.0);
        assert_eq!(clamp_volume(1.4).as_fraction(), 1.0);
    }

    #[test]
    fn volume_of_nothing_is_zero() {
        // undefined and "loud" are both NaN once through Number().
        assert_eq!(clamp_volume(f64::NAN).as_fraction(), 0.0);
        assert_eq!(clamp_volume(f64::INFINITY).as_fraction(), 0.0);
    }

    fn stream(id: &str, playing: bool) -> PlayerRow {
        PlayerRow {
            id: id.into(),
            is_playing: playing,
            auto: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn auto_skips_a_stream_row() {
        let rows = [stream("stream:7", true), bare("a")];
        assert_eq!(pick_player_id(&rows, ""), "a");
        assert_eq!(pick_player_id(&[stream("stream:7", true)], ""), "");
    }

    #[test]
    fn a_stream_row_picked_by_hand_wins() {
        let rows = [row("a", true, ""), stream("stream:7", false)];
        assert_eq!(pick_player_id(&rows, "stream:7"), "stream:7");
    }

    #[test]
    fn labels_carry_kind_and_auto() {
        let rows = with_labels(&[
            PlayerRow {
                id: "radio".into(),
                kind: Some("radio".into()),
                identity: Some("Radio".into()),
                ..Default::default()
            },
            PlayerRow {
                id: "stream:3".into(),
                kind: Some("stream".into()),
                auto: Some(false),
                identity: Some("Discord".into()),
                ..Default::default()
            },
            bare("org.mpris.MediaPlayer2.mpv"),
        ]);
        assert_eq!(rows[0].kind, "radio");
        assert!(!rows[1].auto);
        assert_eq!(rows[2].kind, "mpris");
        assert!(rows[2].auto);
    }

    #[test]
    fn stream_owner_by_bus_word_and_identity() {
        let rows = [
            row(
                "org.mpris.MediaPlayer2.firefox.instance_1_2",
                false,
                "Mozilla Firefox",
            ),
            row("org.mpris.MediaPlayer2.spotify", false, "Spotify"),
        ];
        assert_eq!(
            stream_owner(&[Some("Firefox"), Some("firefox")], &rows),
            "org.mpris.MediaPlayer2.firefox.instance_1_2"
        );
        assert_eq!(
            stream_owner(&[Some("spotify")], &rows),
            "org.mpris.MediaPlayer2.spotify"
        );
        assert_eq!(stream_owner(&[Some("Discord"), Some("discord")], &rows), "");
        assert_eq!(stream_owner(&[None, Some(""), Some("ab")], &rows), "");
    }

    #[test]
    fn sink_input_index_by_object_id() {
        let text = r#"[{"index":41,"properties":{"object.id":"88"}},{"index":52,"properties":{"object.id":"90"}}]"#;
        assert_eq!(sink_input_index(text, 90), 52);
        assert_eq!(sink_input_index(text, 12), -1);
        assert_eq!(sink_input_index("not json", 90), -1);
    }
}
