use serde_json::json;

use super::*;

// fixtures (kopuz's own `line`/`background_line` helpers, ported)

fn line(time: f64, end: Option<f64>, text: &str) -> Line {
    Line::new(time, end, text)
}

fn la(time: f64, end: Option<f64>) -> Line {
    line(time, end, "la")
}

fn background_line(time: f64, end: Option<f64>, parent: Option<usize>) -> Line {
    Line {
        background: true,
        parent,
        ..la(time, end)
    }
}

fn word(time: f64, text: &str, joins_next: bool) -> Word {
    Word {
        time,
        text: text.into(),
        joins_next,
    }
}

fn near(a: f64, b: f64, eps: f64) {
    assert!((a - b).abs() <= eps, "{a} is not within {eps} of {b}");
}

fn interludes(lines: &[Line]) -> Vec<bool> {
    lines.iter().map(|l| l.interlude).collect()
}

// cacheKey

#[test]
fn cache_key_lowercases_and_dashes() {
    assert_eq!(
        cache_key("The Beatles", "Let It Be", "Let It Be", 243.0),
        "the-beatles-let-it-be-let-it-be-243"
    );
}

#[test]
fn cache_key_strips_punctuation() {
    assert_eq!(
        cache_key("Sigur Rós", "( )", "Takk...", 10.0),
        "sigur-ros-takk-10"
    );
}

#[test]
fn cache_key_keeps_non_latin_letters_apart() {
    assert_eq!(cache_key("米津玄師", "Lemon", "", 255.0), "米津玄師-lemon-255");
    assert_ne!(
        cache_key("米津玄師", "馬と鹿", "", 255.0),
        cache_key("米津玄師", "感電", "", 255.0)
    );
}

#[test]
fn cache_key_keeps_dakuten() {
    assert_ne!(cache_key("a", "が", "", 1.0), cache_key("a", "か", "", 1.0));
}

#[test]
fn cache_key_caps_the_slug() {
    let long = "あ".repeat(300);
    assert!(cache_key(&long, "x", "", 1.0).len() <= 210);
}

#[test]
fn cache_key_rounds_the_duration() {
    assert_eq!(cache_key("a", "b", "c", 10.6), "a-b-c-11");
}

#[test]
fn cache_key_of_a_missing_duration_is_zero() {
    assert_eq!(cache_key("a", "b", "c", f64::NAN), "a-b-c-0");
}

// getUrl / searchUrl (lrclib)

#[test]
fn get_url_targets_lrclib_get() {
    let url = get_url("Air", "La Femme d'Argent", "Moon Safari", 429.0);
    assert!(url.starts_with("https://lrclib.net/api/get?"));
    assert!(
        url.contains("track_name=La%20Femme%20d'Argent")
            || url.contains("track_name=La%20Femme%20d%27Argent")
    );
    assert!(url.contains("artist_name=Air"));
}

#[test]
fn get_url_includes_album_name_when_present() {
    assert!(get_url("Air", "Title", "Moon Safari", 429.0).contains("album_name=Moon%20Safari"));
}

#[test]
fn get_url_omits_album_name_when_empty() {
    assert!(!get_url("Air", "Title", "", 429.0).contains("album_name="));
}

#[test]
fn get_url_includes_rounded_duration() {
    assert!(get_url("Air", "Title", "Moon Safari", 428.6).contains("duration=429"));
}

#[test]
fn get_url_omits_duration_when_not_finite() {
    assert!(!get_url("Air", "Title", "Moon Safari", f64::NAN).contains("duration="));
}

#[test]
fn get_url_omits_duration_when_zero_or_negative() {
    assert!(!get_url("Air", "Title", "Moon Safari", 0.0).contains("duration="));
    assert!(!get_url("Air", "Title", "Moon Safari", -5.0).contains("duration="));
}

#[test]
fn search_url_targets_lrclib_search() {
    let url = search_url("Air", "Title");
    assert!(url.starts_with("https://lrclib.net/api/search?"));
    assert!(url.contains("track_name=Title"));
    assert!(url.contains("artist_name=Air"));
}

#[test]
fn search_url_percent_encodes() {
    let url = search_url("Sigur Rós", "Untitled #1");
    assert!(url.contains("Sigur%20R%C3%B3s"));
    assert!(url.contains("Untitled%20%231"));
}

// itunesSearchUrl / paxsenixAppleLyricsUrl / paxsenixYoutubeSearchUrl / paxsenixYoutubeLyricsUrl

#[test]
fn itunes_search_url_orders_title_then_artist() {
    let url = itunes_search_url("Example Artist", "Ninety Two Ten");
    assert!(url.starts_with("https://itunes.apple.com/search?term="));
    assert!(url.contains(&encode_uri_component("Ninety Two Ten Example Artist")));
    assert!(url.contains("entity=song"));
    assert!(url.contains("limit=8"));
    assert!(url.contains("country=US"));
}

#[test]
fn paxsenix_apple_lyrics_url_carries_the_track_id() {
    assert_eq!(
        paxsenix_apple_lyrics_url(12345),
        "https://lyrics.paxsenix.org/apple-music/lyrics?id=12345"
    );
}

#[test]
fn paxsenix_youtube_search_url_orders_title_then_artist() {
    let url = paxsenix_youtube_search_url("Example Artist", "Ninety Two Ten");
    assert!(url.starts_with("https://lyrics.paxsenix.org/youtube/search?q="));
    assert!(url.contains(&encode_uri_component("Ninety Two Ten Example Artist")));
}

#[test]
fn paxsenix_youtube_lyrics_url_carries_the_video_id() {
    assert_eq!(
        paxsenix_youtube_lyrics_url("abc123"),
        "https://lyrics.paxsenix.org/youtube/lyrics?id=abc123"
    );
}

// pickSynced: /api/get object shape

#[test]
fn pick_synced_reads_a_get_hit() {
    let body = json!({ "syncedLyrics": "[00:01.00]Hello\n[00:02.00]World" }).to_string();
    assert_eq!(pick_synced(&body), "[00:01.00]Hello\n[00:02.00]World");
}

#[test]
fn pick_synced_of_a_plain_only_hit_is_empty() {
    let body = json!({ "plainLyrics": "Hello\nWorld", "syncedLyrics": null }).to_string();
    assert_eq!(pick_synced(&body), "");
}

#[test]
fn pick_synced_of_an_instrumental_is_empty() {
    let body =
        json!({ "instrumental": true, "syncedLyrics": null, "plainLyrics": null }).to_string();
    assert_eq!(pick_synced(&body), "");
}

#[test]
fn pick_synced_of_a_synced_lyrics_with_no_usable_timing_is_empty() {
    let body = json!({ "syncedLyrics": "no timestamps here" }).to_string();
    assert_eq!(pick_synced(&body), "");
}

// pickSynced: /api/search array shape

#[test]
fn pick_synced_skips_a_plain_only_hit_for_a_synced_one() {
    let body = json!([
        { "plainLyrics": "Hello", "syncedLyrics": null },
        { "syncedLyrics": "[00:01.00]Hello\n[00:02.00]World" }
    ])
    .to_string();
    assert_eq!(pick_synced(&body), "[00:01.00]Hello\n[00:02.00]World");
}

#[test]
fn pick_synced_of_an_empty_search_array_is_empty() {
    assert_eq!(pick_synced(&json!([]).to_string()), "");
}

// pickSynced: malformed input

#[test]
fn pick_synced_of_an_empty_body_is_empty() {
    assert_eq!(pick_synced(""), "");
}

#[test]
fn pick_synced_of_a_non_json_body_is_empty() {
    assert_eq!(pick_synced("not json{{{"), "");
}

// parseLrc

#[test]
fn parse_lrc_reads_plain_stamps() {
    let lines = parse_lrc("[00:01.00]One\n[00:02.50]Two");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].time, 1.0);
    assert_eq!(lines[0].text, "One");
    assert_eq!(lines[1].time, 2.5);
    assert_eq!(lines[1].text, "Two");
}

#[test]
fn parse_lrc_reads_hundredths_and_thousandths() {
    let lines = parse_lrc("[00:01.5]A\n[00:02.500]B\n[00:03.123]C");
    assert_eq!(lines[0].time, 1.5);
    assert_eq!(lines[1].time, 2.5);
    near(lines[2].time, 3.123, 1e-9);
}

#[test]
fn parse_lrc_reads_minutes() {
    assert_eq!(parse_lrc("[01:02.00]Late")[0].time, 62.0);
}

#[test]
fn parse_lrc_several_stamps_on_one_line_are_one_entry_each() {
    let lines = parse_lrc("[00:01.00][00:05.00]Repeat me");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].time, 1.0);
    assert_eq!(lines[1].time, 5.0);
    assert_eq!(lines[0].text, "Repeat me");
    assert_eq!(lines[1].text, "Repeat me");
}

#[test]
fn parse_lrc_skips_metadata_tags() {
    let lines = parse_lrc("[ar:Some Artist]\n[ti:Some Title]\n[offset:+500]\n[00:01.00]Real line");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "Real line");
}

#[test]
fn parse_lrc_skips_a_stampless_line() {
    assert_eq!(
        parse_lrc("[00:01.00]Real line\nno stamp here at all").len(),
        1
    );
}

#[test]
fn parse_lrc_merges_equal_times_as_a_parenthesised_translation() {
    let lines = parse_lrc("[00:01.00]Hello\n[00:01.00]Bonjour");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "Hello\n(Bonjour)");
}

#[test]
fn parse_lrc_keeps_an_already_parenthesised_translation() {
    assert_eq!(
        parse_lrc("[00:01.00]Hello\n[00:01.00](Bonjour)")[0].text,
        "Hello\n(Bonjour)"
    );
}

#[test]
fn parse_lrc_a_blank_translation_line_adds_nothing() {
    assert_eq!(parse_lrc("[00:01.00]Hello\n[00:01.00] ")[0].text, "Hello");
}

#[test]
fn parse_lrc_sorts_out_of_order_lines() {
    let lines = parse_lrc("[00:05.00]Second\n[00:01.00]First");
    assert_eq!(lines[0].text, "First");
    assert_eq!(lines[1].text, "Second");
}

#[test]
fn parse_lrc_fills_the_new_p1_fields() {
    let l = &parse_lrc("[00:01.00]La")[0];
    assert_eq!(l.end, None);
    assert_eq!(l.parent, None);
    assert!(!l.background);
    assert!(!l.opposite_turn);
    assert!(!l.estimated);
}

#[test]
fn parse_lrc_reads_enhanced_word_stamps() {
    let lines = parse_lrc("[00:01.00]<00:01.00>Hello <00:01.50>world");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "Hello world");
    assert_eq!(
        lines[0].words,
        [word(1.0, "Hello", false), word(1.5, "world", false)]
    );
}

#[test]
fn parse_lrc_groups_syllable_stamps_as_one_word() {
    let lines = parse_lrc("[00:01.00]<00:01.00>Hel<00:01.20>lo <00:01.50>world");
    assert_eq!(lines[0].text, "Hello world");
    assert_eq!(
        lines[0].words,
        [
            word(1.0, "Hel", true),
            word(1.2, "lo", false),
            word(1.5, "world", false)
        ]
    );
    let groups = chunk_words(&lines[0].words);
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0]
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>(),
        ["Hel", "lo"]
    );
    assert_eq!(
        groups[1]
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>(),
        ["world"]
    );
}

// The middle stamp owns nothing (immediately followed by the next stamp), so it
// is dropped from `words`, but the chunk before it still doesn't join across
// it: there was no text there to join to.
#[test]
fn parse_lrc_a_stray_stamp_with_no_text_still_separates() {
    let lines = parse_lrc("[00:01.00]<00:01.00>One<00:01.10><00:01.20>Two");
    assert_eq!(
        lines[0].words,
        [word(1.0, "One", false), word(1.2, "Two", false)]
    );
}

#[test]
fn parse_lrc_of_empty_text_is_empty() {
    assert_eq!(parse_lrc("").len(), 0);
}

// hasUsableTiming

#[test]
fn has_usable_timing_of_one_line_is_true() {
    assert!(has_usable_timing(&[line(1.0, None, "A")]));
}

#[test]
fn has_usable_timing_of_increasing_lines_is_true() {
    assert!(has_usable_timing(&[
        line(1.0, None, "A"),
        line(2.0, None, "B")
    ]));
}

#[test]
fn has_usable_timing_of_lines_stuck_on_one_time_is_false() {
    assert!(!has_usable_timing(&[
        line(1.0, None, "A"),
        line(1.0, None, "B"),
        line(1.0, None, "C")
    ]));
}

#[test]
fn has_usable_timing_of_nothing_is_false() {
    assert!(!has_usable_timing(&[]));
}

// fromPaxsenixApple: spaces, punctuation, part joins

#[test]
fn from_paxsenix_apple_joins_syllable_parts_with_no_space() {
    let body = json!({
        "content": [{
            "text": [
                { "text": "La", "timestamp": 10100, "part": true },
                { "text": "la", "timestamp": 10300, "part": false },
                { "text": "loo", "timestamp": 10600, "part": false }
            ],
            "backgroundText": [],
            "timestamp": 10000,
            "endtime": 11000,
            "background": false,
            "oppositeTurn": false
        }]
    })
    .to_string();
    let lines = from_paxsenix_apple(&body);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].time, 10.0);
    assert_eq!(lines[0].end, Some(11.0));
    assert_eq!(lines[0].text, "Lala loo");
    assert_eq!(
        lines[0].words,
        [
            word(10.1, "La", true),
            word(10.3, "la", false),
            word(10.6, "loo", false)
        ]
    );
    assert_eq!(chunk_words(&lines[0].words).len(), 2);
}

#[test]
fn from_paxsenix_apple_never_spaces_before_punctuation() {
    let body = json!({
        "content": [{
            "text": [
                { "text": "La", "timestamp": 1000, "part": false },
                { "text": ",", "timestamp": 1200, "part": false },
                { "text": "la", "timestamp": 1400, "part": false }
            ],
            "backgroundText": [],
            "timestamp": 1000,
            "endtime": 2000,
            "background": false,
            "oppositeTurn": false
        }]
    })
    .to_string();
    assert_eq!(from_paxsenix_apple(&body)[0].text, "La, la");
}

#[test]
fn from_paxsenix_apple_splits_background_text_with_its_own_parent() {
    let body = json!({
        "content": [
            {
                "text": [
                    { "text": "La", "timestamp": 10000, "part": false },
                    { "text": "la", "timestamp": 10500, "part": false }
                ],
                "backgroundText": [{ "text": "Echo", "timestamp": 10800, "part": false }],
                "timestamp": 10000,
                "endtime": 12000,
                "background": true,
                "oppositeTurn": true
            },
            {
                "text": [{ "text": "Next", "timestamp": 10600, "part": false }],
                "backgroundText": [],
                "timestamp": 10600,
                "endtime": 13000,
                "background": false,
                "oppositeTurn": true
            }
        ]
    })
    .to_string();
    let lines = from_paxsenix_apple(&body);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].text, "La la");
    assert!(!lines[0].background);
    assert!(lines[0].opposite_turn);
    assert_eq!(lines[0].parent, None);
    assert_eq!(lines[1].text, "Echo");
    assert!(lines[1].background);
    assert!(lines[1].opposite_turn);
    assert_eq!(lines[1].parent, Some(0));
    assert_eq!(lines[1].time, 10.8);
    assert_eq!(lines[2].text, "Next");
    assert_eq!(lines[2].parent, None);
}

#[test]
fn from_paxsenix_apple_a_background_row_with_no_background_text_is_itself_background() {
    let body = json!({
        "content": [{
            "text": [{ "text": "Mm", "timestamp": 5000, "part": false }],
            "backgroundText": [],
            "timestamp": 5000,
            "endtime": 5500,
            "background": true,
            "oppositeTurn": false
        }]
    })
    .to_string();
    let lines = from_paxsenix_apple(&body);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].background);
    assert_eq!(lines[0].text, "Mm");
    assert_eq!(lines[0].parent, None);
}

#[test]
fn from_paxsenix_apple_falls_back_to_lrc_when_content_has_no_timing() {
    let body = json!({
        "content": [{
            "text": [{ "text": "Untimed", "part": false }],
            "backgroundText": [],
            "timestamp": 0,
            "endtime": 0,
            "background": false,
            "oppositeTurn": false
        }],
        "lrc": "[00:01.00]One\n[00:02.00]Two",
        "plain": "Untimed"
    })
    .to_string();
    let lines = from_paxsenix_apple(&body);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text, "One");
    assert_eq!(lines[1].text, "Two");
}

#[test]
fn from_paxsenix_apple_of_nothing_usable_is_empty() {
    let body = json!({ "content": [], "lrc": "", "plain": "x" }).to_string();
    assert_eq!(from_paxsenix_apple(&body).len(), 0);
}

#[test]
fn from_paxsenix_apple_of_non_json_is_empty() {
    assert_eq!(from_paxsenix_apple("not json{{{").len(), 0);
}

// matchScore / bestItunesSong / bestYoutubeResult / parseColonDuration

#[test]
fn match_score_of_identical_token_sets_is_one_hundred() {
    assert_eq!(
        match_score(
            "Ninety Two Ten Example Artist",
            "Ninety Two Ten Example Artist"
        ),
        100.0
    );
}

#[test]
fn match_score_reads_non_latin_titles() {
    assert_eq!(match_score("Lemon 米津玄師", "Lemon 米津玄師"), 100.0);
    assert_eq!(match_score("Кино Группа крови", "Группа крови Кино"), 100.0);
    assert_eq!(match_score("馬と鹿 米津玄師", "感電 米津玄師"), 50.0);
}

#[test]
fn match_score_ignores_latin_accents() {
    assert_eq!(match_score("DESPECHÁ ROSALÍA", "Despecha Rosalia"), 100.0);
}

#[test]
fn best_itunes_song_finds_a_japanese_title() {
    let songs = [
        json!({ "trackId": 1, "trackName": "感電", "artistName": "米津玄師", "trackTimeMillis": 266000 }),
        json!({ "trackId": 2, "trackName": "馬と鹿", "artistName": "米津玄師", "trackTimeMillis": 255000 }),
    ];
    let best = best_itunes_song(&songs, "馬と鹿 米津玄師", 255.0).unwrap();
    assert_eq!(best["trackId"], 2);
}

#[test]
fn match_score_of_disjoint_text_is_zero() {
    assert_eq!(
        match_score("Completely Different Words", "Nothing Alike Here"),
        0.0
    );
}

#[test]
fn match_score_strips_the_feat_marker_without_dropping_the_name() {
    assert_eq!(
        match_score("Song (feat. Other) Artist", "Song Artist"),
        match_score("Song Other Artist", "Song Artist")
    );
}

#[test]
fn best_itunes_song_drops_candidates_below_the_floor() {
    let songs = [json!({ "trackId": 1, "trackName": "Nothing Alike", "artistName": "Unrelated" })];
    assert_eq!(
        best_itunes_song(&songs, "Ninety Two Ten Example Artist", 0.0),
        None
    );
}

#[test]
fn best_itunes_song_prefers_the_duration_match_within_the_window() {
    let songs = [
        json!({ "trackId": 1, "trackName": "Ninety Two Ten", "artistName": "Example Artist", "trackTimeMillis": 230000 }),
        json!({ "trackId": 2, "trackName": "Ninety Two Ten", "artistName": "Example Artist", "trackTimeMillis": 197200 }),
    ];
    let best = best_itunes_song(&songs, "Ninety Two Ten Example Artist", 198.0).unwrap();
    assert_eq!(best["trackId"], 2);
}

#[test]
fn best_itunes_song_rejects_a_candidate_past_the_duration_window() {
    let songs = [
        json!({ "trackId": 1, "trackName": "Ninety Two Ten", "artistName": "Example Artist", "trackTimeMillis": 400000 }),
    ];
    assert_eq!(
        best_itunes_song(&songs, "Ninety Two Ten Example Artist", 198.0),
        None
    );
}

#[test]
fn best_youtube_result_checks_the_duration_window() {
    let results = [
        json!({ "videoId": "wrong-duration", "title": "Ninety Two Ten", "author": "Example Artist", "duration": "5:40" }),
        json!({ "videoId": "right-duration", "title": "Ninety Two Ten", "author": "Example Artist", "duration": "3:28" }),
    ];
    let best = best_youtube_result(&results, "Ninety Two Ten Example Artist", 208.0).unwrap();
    assert_eq!(best["videoId"], "right-duration");
}

#[test]
fn parse_colon_duration_reads_minutes_and_seconds() {
    assert_eq!(parse_colon_duration("3:28"), Some(208.0));
}

#[test]
fn parse_colon_duration_reads_hours() {
    assert_eq!(parse_colon_duration("1:02:03"), Some(3723.0));
}

#[test]
fn parse_colon_duration_of_empty_is_none() {
    assert_eq!(parse_colon_duration(""), None);
}

#[test]
fn parse_colon_duration_of_non_numeric_is_none() {
    assert_eq!(parse_colon_duration("nope"), None);
}

// quality / pickBest / isDefinitive

fn two_words() -> Vec<Word> {
    vec![word(1.0, "a", false), word(1.5, "b", false)]
}

#[test]
fn quality_of_nothing_is_zero() {
    assert_eq!(quality(&[]), 0);
}

#[test]
fn quality_of_line_timing_only_is_one() {
    assert_eq!(quality(&[line(1.0, None, "")]), 1);
}

#[test]
fn quality_of_word_timing_is_two() {
    assert_eq!(
        quality(&[Line {
            words: two_words(),
            ..line(1.0, None, "")
        }]),
        2
    );
}

#[test]
fn is_definitive_matches_quality_two() {
    assert!(is_definitive(&[Line {
        words: two_words(),
        ..line(1.0, None, "")
    }]));
    assert!(!is_definitive(&[line(1.0, None, "")]));
}

#[test]
fn pick_best_prefers_the_highest_quality() {
    let results = [
        Candidate {
            source: "lrclib".into(),
            lines: vec![line(1.0, None, "")],
        },
        Candidate {
            source: "apple".into(),
            lines: vec![Line {
                words: two_words(),
                ..line(1.0, None, "")
            }],
        },
    ];
    assert_eq!(pick_best(&results).unwrap().source, "apple");
}

#[test]
fn pick_best_ties_go_to_the_earlier_entry() {
    let results = [
        Candidate {
            source: "apple".into(),
            lines: vec![line(1.0, None, "")],
        },
        Candidate {
            source: "youtube".into(),
            lines: vec![line(1.0, None, "")],
        },
    ];
    assert_eq!(pick_best(&results).unwrap().source, "apple");
}

#[test]
fn pick_best_of_all_quality_zero_is_none() {
    let results = [
        Candidate {
            source: "apple".into(),
            lines: vec![],
        },
        Candidate {
            source: "lrclib".into(),
            lines: vec![],
        },
    ];
    assert_eq!(pick_best(&results), None);
}

// chunkGlow

#[test]
fn chunk_glow_is_full_while_the_chunk_is_being_sung() {
    let words = [word(1.0, "Hel", true), word(2.0, "lo", false)];
    assert_eq!(chunk_glow(&words, 0, Some(3.0), 0.5), 0.0);
    assert_eq!(chunk_glow(&words, 0, Some(3.0), 1.5), 1.0);
    assert_eq!(chunk_glow(&words, 0, Some(3.0), 2.0), 1.0);
}

#[test]
fn chunk_glow_decays_linearly_once_the_chunk_is_over() {
    let words = [word(1.0, "Hel", true), word(2.0, "lo", false)];
    assert_eq!(
        chunk_glow(&words, 0, Some(3.0), 2.0 + GLOW_DECAY_SECONDS / 2.0),
        0.5
    );
    assert_eq!(
        chunk_glow(&words, 0, Some(3.0), 2.0 + GLOW_DECAY_SECONDS),
        0.0
    );
    assert_eq!(chunk_glow(&words, 0, Some(3.0), 10.0), 0.0);
}

// chunkWords

#[test]
fn chunk_words_one_chunk_per_word_for_a_word_stamped_line() {
    let words = [word(1.0, "Hello", false), word(1.5, "world", false)];
    let groups = chunk_words(&words);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].len(), 1);
    assert_eq!(groups[1].len(), 1);
}

#[test]
fn chunk_words_of_nothing_is_empty() {
    assert_eq!(chunk_words(&[]).len(), 0);
}

// chunkEnd / chunkProgress

fn ab(a: f64, b: f64) -> Vec<Word> {
    vec![word(a, "A", false), word(b, "B", false)]
}

#[test]
fn chunk_end_runs_to_the_next_chunk() {
    assert_eq!(chunk_end(&ab(1.0, 2.0), 0, Some(10.0)), 2.0);
}

#[test]
fn chunk_end_of_the_last_chunk_runs_to_the_line_end() {
    assert_eq!(chunk_end(&[word(1.0, "A", false)], 0, Some(4.0)), 4.0);
}

#[test]
fn chunk_end_of_the_last_chunk_with_no_line_end_falls_back() {
    assert_eq!(
        chunk_end(&[word(1.0, "A", false)], 0, None),
        1.0 + WORD_FALLBACK_SECONDS
    );
}

#[test]
fn chunk_progress_before_its_stamp_is_zero() {
    assert_eq!(chunk_progress(&ab(5.0, 6.0), 0, None, 4.0, false), 0.0);
}

#[test]
fn chunk_progress_partway_through_its_span() {
    assert_eq!(chunk_progress(&ab(5.0, 6.0), 0, None, 5.5, false), 0.5);
}

#[test]
fn chunk_progress_after_its_span_is_one() {
    assert_eq!(chunk_progress(&ab(5.0, 6.0), 0, None, 100.0, false), 1.0);
}

// The next chunk is 10s out; the wipe still finishes at the cap rather than
// creeping toward it.
#[test]
fn chunk_progress_caps_the_span_at_wipe_max_seconds() {
    let words = ab(0.0, 10.0);
    assert_eq!(
        chunk_progress(&words, 0, None, WIPE_MAX_SECONDS, false),
        1.0
    );
    assert!(chunk_progress(&words, 0, None, WIPE_MAX_SECONDS / 2.0, false) < 1.0);
}

#[test]
fn chunk_progress_of_the_last_chunk_runs_to_the_line_end() {
    let words = [word(0.0, "A", false)];
    assert_eq!(chunk_progress(&words, 0, Some(0.5), 0.5, false), 1.0);
    assert_eq!(chunk_progress(&words, 0, Some(0.5), 0.25, false), 0.5);
}

// The cap is a provider's own stamp held over a pause. A synthesised chunk has
// no pause in it, so it keeps its whole share of the line.
#[test]
fn chunk_progress_of_a_synthesised_chunk_is_not_capped() {
    let words = ab(0.0, 10.0);
    assert_eq!(chunk_progress(&words, 0, None, 5.0, true), 0.5);
    assert_eq!(chunk_progress(&words, 0, None, 10.0, true), 1.0);
}

// Two line-synced lines of the same words held for different lengths wipe at
// their own rates, each reaching the end of its text as the next line lights,
// rather than both running at one fixed rate.
fn estimated_wipe(line_time: f64, next_time: f64, t: f64) -> f64 {
    let lines = synthesise_words(&display_lines(&[
        line(line_time, None, "one two"),
        la(next_time, None),
    ]));
    let words = &lines[usize::from(line_time > SEAMLESS_GAP_SECONDS)].words;
    let sum: f64 = (0..words.len())
        .map(|i| chunk_progress(words, i, Some(next_time), t, true))
        .sum();
    sum / words.len() as f64
}

// The real line-synced path, end to end: a plain lrclib body with no end stamps
// and no word tags, parsed by the same `parse_lrc` the providers and the
// sibling `.lrc` both go through, spliced and synthesised the way the service
// publishes it. `lrc_wipe` is the fraction of the whole line wiped at `t`, off
// equal-length words so the fraction is the line's own clock and nothing else.
fn lrc_lines(body: &str) -> Vec<Line> {
    synthesise_words(&display_lines(&parse_lrc(body)))
}

fn lrc_wipe(lines: &[Line], index: usize, t: f64) -> f64 {
    let l = &lines[index];
    let line_end = finite(l.end).or_else(|| lines.get(index + 1).map(|n| n.time));
    let sum: f64 = (0..l.words.len())
        .map(|i| chunk_progress(&l.words, i, line_end, t, l.estimated))
        .sum();
    sum / l.words.len() as f64
}

// A three-second line and a ten-second one, same four words, from one
// real-shaped body. Both wipe over their own span: the quarter marks land at a
// quarter of each line's own length, and the wipe reaches the end of the text
// as the next line lights.
#[test]
fn a_line_synced_wipe_spans_each_line_it_is_held_for() {
    let lines = lrc_lines("[00:01.00]one two six ten\n[00:04.00]one two six ten\n[00:14.00]last\n");
    assert_eq!(lines.len(), 3);
    assert!(lines[0].estimated);
    assert!(lines[1].estimated);

    near(lrc_wipe(&lines, 0, 1.75), 0.25, 0.02);
    near(lrc_wipe(&lines, 0, 2.5), 0.5, 0.02);
    near(lrc_wipe(&lines, 0, 3.25), 0.75, 0.02);
    assert_eq!(lrc_wipe(&lines, 0, 4.0), 1.0);

    near(lrc_wipe(&lines, 1, 6.5), 0.25, 0.02);
    near(lrc_wipe(&lines, 1, 9.0), 0.5, 0.02);
    near(lrc_wipe(&lines, 1, 11.5), 0.75, 0.02);
    assert_eq!(lrc_wipe(&lines, 1, 14.0), 1.0);
}

// And where the gap is wide enough to be marked, the line is lit up to the note
// and no further, so the wipe still lands as the row changes hands rather than
// running on into a stretch nobody is singing.
#[test]
fn a_line_synced_wipe_ends_where_the_note_takes_over() {
    let lines = lrc_lines("[00:01.00]one two six ten\n[00:21.00]last\n");
    assert_eq!(lines.len(), 3);
    assert!(lines[1].interlude);
    assert_eq!(lines[1].time, 1.0 + LINE_ASSUMED_SECONDS);

    near(lrc_wipe(&lines, 0, 4.5), 0.5, 0.02);
    assert_eq!(lrc_wipe(&lines, 0, 8.0), 1.0);
}

#[test]
fn an_estimated_wipe_tracks_the_line_it_is_held_for() {
    // Held 2s and held 6s, each sampled at half its own span.
    near(estimated_wipe(0.0, 2.0, 1.0), 0.5, 0.06);
    near(estimated_wipe(0.0, 6.0, 3.0), 0.5, 0.06);
    // At one wall-clock second the short line is half sung and the long one has
    // barely started, which is the difference a fixed rate cannot draw.
    assert!(estimated_wipe(0.0, 2.0, 1.0) > estimated_wipe(0.0, 6.0, 1.0) + 0.25);
    // And three quarters through the long line it is still travelling, where a
    // capped chunk would have finished and be waiting.
    assert!(estimated_wipe(0.0, 6.0, 4.5) < 0.9);
}

#[test]
fn an_estimated_wipe_lands_as_the_next_line_lights() {
    assert_eq!(estimated_wipe(0.0, 2.0, 2.0), 1.0);
    assert_eq!(estimated_wipe(0.0, 6.0, 6.0), 1.0);
}

// synthesiseWords

#[test]
fn synthesise_words_shares_the_span_by_character_count() {
    let out = synthesise_words(&[line(0.0, Some(11.0), "one two three")]);
    assert!(out[0].estimated);
    assert_eq!(
        out[0].words,
        [
            word(0.0, "one", false),
            word(3.0, "two", false),
            word(6.0, "three", false)
        ]
    );
}

#[test]
fn synthesise_words_spans_to_the_next_main_line_when_untimed() {
    let out = synthesise_words(&[line(0.0, None, "la la"), line(4.0, None, "la")]);
    assert_eq!(out[0].words[0].time, 0.0);
    assert_eq!(out[0].words[1].time, 2.0);
}

#[test]
fn synthesise_words_leaves_a_provider_timed_line_alone() {
    let lines = [Line {
        words: vec![word(0.0, "la", false)],
        ..line(0.0, Some(2.0), "la")
    }];
    assert_eq!(synthesise_words(&lines)[0], lines[0]);
}

#[test]
fn synthesise_words_skips_an_interlude() {
    let out = synthesise_words(&[Line {
        interlude: true,
        ..line(0.0, Some(5.0), "")
    }]);
    assert_eq!(out[0].words.len(), 0);
    assert!(!out[0].estimated);
}

#[test]
fn synthesise_words_skips_a_merged_translation() {
    let out = synthesise_words(&[line(0.0, Some(3.0), "la\n(la)")]);
    assert_eq!(out[0].words.len(), 0);
}

// mainLineIndices / nextMainLineStart / lineEndEstimate

#[test]
fn main_line_indices_are_every_line_when_none_are_background() {
    assert_eq!(main_line_indices(&[la(1.0, None), la(2.0, None)]), [0, 1]);
}

#[test]
fn main_line_indices_skip_background_lines() {
    let lines = [
        la(1.0, None),
        background_line(1.5, Some(2.0), Some(0)),
        la(3.0, None),
    ];
    assert_eq!(main_line_indices(&lines), [0, 2]);
}

#[test]
fn main_line_indices_fall_back_to_every_line_when_all_are_background() {
    let lines = [
        background_line(1.0, Some(2.0), None),
        background_line(3.0, Some(4.0), None),
    ];
    assert_eq!(main_line_indices(&lines), [0, 1]);
}

#[test]
fn next_main_line_start_of_the_last_main_line_is_none() {
    let lines = [la(1.0, None), la(2.0, None)];
    assert_eq!(
        next_main_line_start(&lines, &main_line_indices(&lines), 1),
        None
    );
}

// A track switch republishes `lines` while a caller still holds a
// `main_indices` built from the longer, previous array. An index past the
// current array is a stale read, not a corrupt one, so every one of these
// skips it.
#[test]
fn active_main_line_index_ignores_a_main_indices_entry_past_the_current_lines() {
    let lines = [la(1.0, None), la(2.0, None)];
    assert_eq!(active_main_line_index(&lines, &[0, 1, 2, 5], 2.5), Some(1));
}

#[test]
fn next_main_line_start_of_a_main_indices_entry_past_the_current_lines_is_none() {
    let lines = [la(1.0, None), la(2.0, None)];
    assert_eq!(next_main_line_start(&lines, &[0, 5], 0), None);
}

#[test]
fn background_line_bound_ignores_a_main_indices_entry_past_the_current_lines() {
    let lines = [la(1.0, None), background_line(1.5, None, Some(0))];
    assert_eq!(background_line_bound(&lines, &[0, 5], &lines[1]), None);
}

#[test]
fn line_end_estimate_prefers_its_own_end() {
    assert_eq!(line_end_estimate(&la(1.0, Some(4.0))), 4.0);
}

#[test]
fn line_end_estimate_falls_back_to_the_last_word() {
    let l = Line {
        words: vec![word(1.0, "la", false), word(2.0, "la", false)],
        ..la(1.0, None)
    };
    assert_eq!(line_end_estimate(&l), 2.0 + WORD_FALLBACK_SECONDS);
}

#[test]
fn line_end_estimate_falls_back_to_the_assumed_tail() {
    assert_eq!(
        line_end_estimate(&la(1.0, None)),
        1.0 + LINE_ASSUMED_SECONDS
    );
}

// ledPosition

#[test]
fn led_position_reads_ahead_of_the_player() {
    near(led_position(10.0, 0.0), 10.0 + POSITION_LEAD_SECONDS, 1e-9);
}

#[test]
fn led_position_stacks_the_offset_on_the_lead() {
    near(
        led_position(10.0, 0.25),
        10.0 + POSITION_LEAD_SECONDS - 0.25,
        1e-9,
    );
    near(
        led_position(10.0, -0.25),
        10.0 + POSITION_LEAD_SECONDS + 0.25,
        1e-9,
    );
}

#[test]
fn led_position_without_an_offset_is_the_lead_alone() {
    near(
        led_position(10.0, f64::NAN),
        10.0 + POSITION_LEAD_SECONDS,
        1e-9,
    );
}

#[test]
fn a_line_lights_a_lead_before_its_own_stamp() {
    let lines = [la(10.0, Some(14.0)), la(14.0, Some(18.0))];
    let main = main_line_indices(&lines);
    assert_eq!(
        active_main_line_index(&lines, &main, led_position(9.95, 0.0)),
        Some(0)
    );
    assert_eq!(
        active_main_line_index(&lines, &main, led_position(9.85, 0.0)),
        None
    );
}

#[test]
fn a_chunk_is_already_wiping_at_its_own_stamp() {
    near(
        chunk_progress(&ab(5.0, 6.0), 0, None, led_position(5.0, 0.0), false),
        POSITION_LEAD_SECONDS,
        1e-9,
    );
}

#[test]
fn the_offset_key_can_hold_a_chunk_back_past_its_stamp() {
    assert_eq!(
        chunk_progress(&ab(5.0, 6.0), 0, None, led_position(5.0, 0.3), false),
        0.0
    );
}

// holdSeconds / outputLatency

#[test]
fn hold_is_the_latency_plus_the_manual_nudge() {
    near(hold_seconds(true, 250.0, 0.0), 0.25, 1e-9);
    near(hold_seconds(true, 250.0, -50.0), 0.2, 1e-9);
}

#[test]
fn hold_without_auto_is_the_manual_nudge_alone() {
    near(hold_seconds(false, 250.0, 120.0), 0.12, 1e-9);
    assert_eq!(hold_seconds(false, 250.0, f64::NAN), 0.0);
}

#[test]
fn a_hold_moves_the_lit_line_by_the_latency() {
    let lines = [la(10.0, Some(14.0)), la(14.0, Some(18.0))];
    let main = main_line_indices(&lines);
    let hold = hold_seconds(true, 250.0, 0.0);
    assert_eq!(
        active_main_line_index(&lines, &main, led_position(10.0, 0.0)),
        Some(0)
    );
    assert_eq!(
        active_main_line_index(&lines, &main, led_position(10.0, hold)),
        None
    );
    assert_eq!(
        active_main_line_index(&lines, &main, led_position(10.15, hold)),
        Some(0)
    );
}

#[test]
fn the_streams_win_over_the_sink() {
    assert_eq!(
        output_latency([180.0, 21.3], Some(21.3)),
        OutputLatency {
            ms: 180,
            via: LatencyVia::Stream
        }
    );
}

#[test]
fn no_stream_falls_back_to_the_sink() {
    assert_eq!(
        output_latency([], Some(21.33)),
        OutputLatency {
            ms: 21,
            via: LatencyVia::Sink
        }
    );
}

#[test]
fn nothing_declared_is_no_latency() {
    assert_eq!(
        output_latency([], None),
        OutputLatency {
            ms: 0,
            via: LatencyVia::None,
        }
    );
}

#[test]
fn a_misreported_latency_is_capped() {
    assert_eq!(
        output_latency([], Some(9000.0)).ms,
        OUTPUT_LATENCY_MAX_MS as u32
    );
}

// lineActiveAt / activeMainLineIndex / backgroundLineBound / activeSecondaryLines
// (kopuz's own test cases, ported by name)

// The next main line begins while the previous one's backing vocal is still
// running.
#[test]
fn background_line_stays_lit_when_the_next_main_line_starts() {
    let lines = [
        la(63.167, Some(67.299)),
        background_line(65.48, Some(67.299), Some(0)),
        la(66.236, Some(70.567)),
    ];
    let main = main_line_indices(&lines);
    assert_eq!(active_main_line_index(&lines, &main, 66.5), Some(2));
    assert_eq!(active_secondary_lines(&lines, &main, 66.5, Some(2)), [0, 1]);
    assert_eq!(
        active_secondary_lines(&lines, &main, 67.5, Some(2)),
        Vec::<usize>::new()
    );
}

#[test]
fn background_line_stays_lit_after_its_parent_ends_with_no_main_line_active() {
    let lines = [
        la(1.0, Some(2.0)),
        background_line(1.5, Some(5.0), Some(0)),
        la(10.0, Some(12.0)),
    ];
    let main = main_line_indices(&lines);
    assert_eq!(active_main_line_index(&lines, &main, 3.0), None);
    assert_eq!(active_secondary_lines(&lines, &main, 3.0, None), [1]);
    assert_eq!(
        active_secondary_lines(&lines, &main, 5.5, None),
        Vec::<usize>::new()
    );
}

#[test]
fn untimed_background_line_runs_until_the_next_main_line() {
    let lines = [
        la(1.0, Some(2.0)),
        background_line(1.5, None, Some(0)),
        la(4.0, Some(6.0)),
    ];
    let main = main_line_indices(&lines);
    assert_eq!(active_secondary_lines(&lines, &main, 3.0, None), [1]);
    assert_eq!(
        active_secondary_lines(&lines, &main, 4.5, Some(2)),
        Vec::<usize>::new()
    );
}

// displayLines (kopuz's build_display_lines, ported by name)

#[test]
fn marks_intro_and_instrumental_gaps() {
    let lines = [
        la(12.0, Some(15.0)),
        la(40.0, Some(43.0)),
        la(45.0, Some(48.0)),
    ];
    let display = display_lines(&lines);
    assert_eq!(display.len(), 5);
    assert_eq!(interludes(&display), [true, false, true, false, false]);
    assert_eq!(display[0].time, 0.0);
    assert_eq!(display[0].end, Some(12.0));
    assert_eq!(display[2].time, 15.0);
    assert_eq!(display[2].end, Some(40.0));
}

#[test]
fn gap_starts_after_a_background_line_outlasts_its_parent() {
    let lines = [
        la(1.0, Some(4.0)),
        background_line(3.0, Some(9.0), Some(0)),
        la(30.0, Some(33.0)),
    ];
    let display = display_lines(&lines);
    assert_eq!(interludes(&display), [false, false, true, false]);
    assert_eq!(display[2].time, 9.0);
    assert_eq!(display[3].parent, None);
    assert_eq!(display[1].parent, Some(0));
}

#[test]
fn leaves_lyrics_untouched_without_a_long_gap() {
    let lines = [la(1.0, Some(4.0)), la(5.0, Some(8.0))];
    let display = display_lines(&lines);
    assert_eq!(display, lines);
    assert_eq!(interludes(&display), [false, false]);
}

#[test]
fn untimed_lines_fall_back_to_an_assumed_tail() {
    let display = display_lines(&[la(0.0, None), la(60.0, None)]);
    assert_eq!(interludes(&display), [false, true, false]);
    assert_eq!(display[1].time, LINE_ASSUMED_SECONDS);
    assert_eq!(display[1].end, Some(60.0));
}

#[test]
fn display_lines_of_nothing_is_empty() {
    assert_eq!(display_lines(&[]).len(), 0);
}

// The dark rule: a run that ends four seconds before the next line goes dark,
// since the seamless carry only reaches three, and a dark pane has to carry the
// note. kopuz's own five-second threshold would leave this one drawing nothing
// at all.
#[test]
fn marks_a_gap_the_seamless_carry_cannot_hold() {
    let display = display_lines(&[la(1.0, Some(5.0)), la(9.0, Some(13.0))]);
    assert_eq!(interludes(&display), [false, true, false]);
    assert_eq!(display[1].time, 5.0);
    assert_eq!(display[1].end, Some(9.0));
}

#[test]
fn leaves_a_gap_the_seamless_carry_holds() {
    assert_eq!(
        display_lines(&[la(1.0, Some(5.0)), la(8.0, Some(12.0))]).len(),
        2
    );
}

// The note lights exactly where the line before it goes dark, which is the
// whole point of the rule: the two answers can never disagree.
#[test]
fn a_marked_gap_starts_where_the_line_stops_being_lit() {
    let display = display_lines(&[la(1.0, Some(5.0)), la(9.0, Some(13.0))]);
    let main = main_line_indices(&display);
    assert_eq!(active_main_line_index(&display, &main, 4.9), Some(0));
    assert_eq!(active_main_line_index(&display, &main, 5.1), Some(1));
    assert!(display[1].interlude);
}

// Nothing is lit before the first line either, so a run-in past the carry takes
// the note too.
#[test]
fn marks_a_run_in_past_the_seamless_carry() {
    let display = display_lines(&[la(4.0, Some(8.0))]);
    assert_eq!(interludes(&display), [true, false]);
    assert_eq!(display[0].end, Some(4.0));
    assert_eq!(display_lines(&[la(3.0, Some(8.0))]).len(), 1);
}

// A run with no end of its own never goes dark (`line_active_at` holds it to
// the next main line), so the dark rule has nothing to mark and only kopuz's
// own threshold speaks.
#[test]
fn a_run_with_no_end_takes_only_the_assumed_rule() {
    assert_eq!(display_lines(&[la(1.0, None), la(5.0, None)]).len(), 2);

    let wide = display_lines(&[la(1.0, None), la(21.0, None)]);
    assert_eq!(interludes(&wide), [false, true, false]);
    assert_eq!(wide[1].time, 1.0 + LINE_ASSUMED_SECONDS);
}

// Synthesised words are spread over the very estimate this reads, so taking
// them as the line's end would close every instrumental gap on a line-synced
// track to nothing and the note would never appear.
#[test]
fn synthesised_words_do_not_stand_in_for_a_line_end() {
    let lines = synthesise_words(&[line(1.0, None, "one two three"), la(21.0, None)]);
    assert!(lines[0].estimated);
    assert_eq!(line_end_estimate(&lines[0]), 1.0 + LINE_ASSUMED_SECONDS);

    let display = display_lines(&lines);
    assert_eq!(interludes(&display), [false, true, false]);
    assert_eq!(display[1].time, 1.0 + LINE_ASSUMED_SECONDS);
}

// depthOpacity

#[test]
fn depth_opacity_of_the_active_line_is_full() {
    assert_eq!(depth_opacity(0.0, None), 1.0);
}

#[test]
fn depth_opacity_falls_with_distance() {
    assert_eq!(depth_opacity(1.0, None), 0.7);
    assert_eq!(depth_opacity(-1.0, None), 0.7);
    assert_eq!(depth_opacity(2.0, None), 0.45);
    assert_eq!(depth_opacity(3.0, None), 0.25);
}

#[test]
fn depth_opacity_floors_at_three_lines_away() {
    assert_eq!(depth_opacity(4.0, None), 0.25);
    assert_eq!(depth_opacity(-10.0, None), 0.25);
}

// A pane with room for nine rows under the anchor spends the same four table
// entries over those nine, so the sixth row down is still readable instead of
// sitting on the floor with three rows of the pane to spare.
#[test]
fn depth_opacity_spreads_over_the_rows_that_fit() {
    assert_eq!(depth_opacity(3.0, Some(9.0)), 0.7);
    assert_eq!(depth_opacity(6.0, Some(9.0)), 0.45);
    assert_eq!(depth_opacity(9.0, Some(9.0)), 0.25);
}

#[test]
fn depth_opacity_floors_at_the_end_of_its_span() {
    assert_eq!(depth_opacity(12.0, Some(9.0)), 0.25);
}

#[test]
fn depth_opacity_of_a_shallow_pane_falls_faster() {
    assert_eq!(depth_opacity(1.0, Some(1.5)), 0.45);
}

// rowSpans

#[test]
fn row_spans_split_the_viewport_at_the_comfort_offset() {
    let spans = row_spans(400.0, 40.0);
    assert_eq!(spans.above, 4.2);
    assert_eq!(spans.below, 5.8);
}

#[test]
fn row_spans_never_fall_under_one_row() {
    let spans = row_spans(40.0, 40.0);
    assert_eq!(spans.above, 1.0);
    assert_eq!(spans.below, 1.0);
}

#[test]
fn row_spans_of_an_unmeasured_pane_are_one() {
    let spans = row_spans(0.0, 0.0);
    assert_eq!(spans.above, 1.0);
    assert_eq!(spans.below, 1.0);
}

// edgeFraction

#[test]
fn edge_fraction_clear_of_both_ends_is_one() {
    assert_eq!(edge_fraction(30.0, 20.0, 100.0, None), 1.0);
}

#[test]
fn edge_fraction_half_a_row_from_the_top_is_half() {
    assert_eq!(edge_fraction(10.0, 20.0, 100.0, None), 0.5);
}

#[test]
fn edge_fraction_half_a_row_from_the_bottom_is_half() {
    assert_eq!(edge_fraction(70.0, 20.0, 100.0, None), 0.5);
}

// The row the clip would cut is already gone: the ramp runs out as the end of
// the viewport reaches the row, not as it crosses it.
#[test]
fn edge_fraction_touching_an_end_is_zero() {
    assert_eq!(edge_fraction(0.0, 20.0, 100.0, None), 0.0);
    assert_eq!(edge_fraction(80.0, 20.0, 100.0, None), 0.0);
}

#[test]
fn edge_fraction_fully_out_above_is_zero() {
    assert_eq!(edge_fraction(-30.0, 20.0, 100.0, None), 0.0);
}

#[test]
fn edge_fraction_fully_out_below_is_zero() {
    assert_eq!(edge_fraction(110.0, 20.0, 100.0, None), 0.0);
}

#[test]
fn edge_fraction_of_a_zero_height_item_is_zero() {
    assert_eq!(edge_fraction(10.0, 0.0, 100.0, None), 0.0);
}

// A wrapped line is three rows tall and rests where every other line rests, so
// the fade it carries there is one row's worth, not its own.
#[test]
fn edge_fraction_is_spent_over_the_ramp_it_is_given() {
    assert_eq!(edge_fraction(30.0, 60.0, 200.0, Some(20.0)), 1.0);
    assert_eq!(edge_fraction(10.0, 60.0, 200.0, Some(20.0)), 0.5);
    assert_eq!(edge_fraction(0.0, 60.0, 200.0, Some(20.0)), 0.0);
    assert_eq!(edge_fraction(150.0, 60.0, 200.0, Some(20.0)), 0.0);
}

// blurFor

#[test]
fn blur_for_of_the_anchor_is_zero() {
    assert_eq!(blur_for(0.0, 100.0, None), 0.0);
}

#[test]
fn blur_for_scales_by_distance_and_quantises() {
    assert_eq!(blur_for(2.0, 100.0, None), 2.0);
}

#[test]
fn blur_for_caps_before_scaling_by_strength() {
    assert_eq!(blur_for(10.0, 100.0, None), 6.0);
}

#[test]
fn blur_for_scales_by_strength_percent() {
    assert_eq!(blur_for(5.0, 50.0, None), 3.0);
}

#[test]
fn blur_for_of_zero_strength_is_zero() {
    assert_eq!(blur_for(5.0, 0.0, None), 0.0);
}

#[test]
fn blur_for_caps_at_the_end_of_its_own_span() {
    assert_eq!(blur_for(10.0, 100.0, Some(10.0)), 6.0);
    assert_eq!(blur_for(5.0, 100.0, Some(10.0)), 3.0);
}

// chunkRowBands

#[test]
fn chunk_row_bands_of_an_unmeasured_chunk_cover_the_box() {
    let bands = chunk_row_bands(&[], 20.0, 120.0);
    assert_eq!(bands.len(), 1);
    assert_eq!(
        bands[0],
        RowBand {
            top: 0.0,
            height: 20.0,
            width: 120.0
        }
    );
}

// The bands tile the box: the first starts at its top whatever the font's own
// first baseline offset is, and the last runs to its floor, so no sliver of a
// glyph is left outside the mask.
#[test]
fn chunk_row_bands_tile_the_whole_box() {
    let bands = chunk_row_bands(
        &[
            TextRow {
                y: 2.0,
                height: 18.0,
                width: 300.0,
            },
            TextRow {
                y: 20.0,
                height: 18.0,
                width: 140.0,
            },
        ],
        40.0,
        320.0,
    );
    assert_eq!(bands.len(), 2);
    assert_eq!(
        bands[0],
        RowBand {
            top: 0.0,
            height: 20.0,
            width: 300.0
        }
    );
    assert_eq!(
        bands[1],
        RowBand {
            top: 20.0,
            height: 20.0,
            width: 140.0
        }
    );
}

// rowWipe

fn two_bands() -> [RowBand; 2] {
    [
        RowBand {
            top: 0.0,
            height: 20.0,
            width: 300.0,
        },
        RowBand {
            top: 20.0,
            height: 20.0,
            width: 100.0,
        },
    ]
}

#[test]
fn row_wipe_of_a_single_row_is_the_chunk_progress() {
    assert_eq!(
        row_wipe(
            &[RowBand {
                top: 0.0,
                height: 20.0,
                width: 120.0
            }],
            0,
            0.4
        ),
        0.4
    );
}

// Reading order across a break: the first row is finishing while the second has
// not started, which is the whole defect.
#[test]
fn row_wipe_finishes_a_row_before_the_next_one_starts() {
    let bands = two_bands();
    assert_eq!(row_wipe(&bands, 0, 0.5), 2.0 / 3.0);
    assert_eq!(row_wipe(&bands, 1, 0.5), 0.0);
}

#[test]
fn row_wipe_starts_the_second_row_once_the_first_is_full() {
    let bands = two_bands();
    assert_eq!(row_wipe(&bands, 0, 0.75), 1.0);
    assert_eq!(row_wipe(&bands, 1, 0.75), 0.0);
    assert_eq!(row_wipe(&bands, 1, 0.875), 0.5);
}

// The edge travels at one speed over the chunk's whole ink, so a short last row
// takes proportionally less of the chunk's own span than a full one rather than
// an equal share of it.
#[test]
fn row_wipe_weights_a_row_by_its_own_ink_width() {
    let bands = two_bands();
    assert_eq!(row_wipe(&bands, 0, 1.0), 1.0);
    assert_eq!(row_wipe(&bands, 1, 1.0), 1.0);
    assert_eq!(row_wipe(&bands, 0, 0.0), 0.0);
    assert_eq!(row_wipe(&bands, 1, 0.0), 0.0);
}

#[test]
fn row_wipe_of_a_row_that_is_not_there_is_zero() {
    assert_eq!(
        row_wipe(
            &[RowBand {
                top: 0.0,
                height: 20.0,
                width: 120.0
            }],
            3,
            1.0
        ),
        0.0
    );
    assert_eq!(row_wipe(&[], 0, 1.0), 0.0);
}

// comfortY

#[test]
fn comfort_y_rests_the_item_top_at_the_comfort_fraction() {
    assert_eq!(comfort_y(200.0, 50.0), 34.0);
}

#[test]
fn comfort_y_of_an_item_already_there_is_zero() {
    assert_eq!(comfort_y(100.0, 42.0), 0.0);
}

#[test]
fn a_line_round_trips_through_the_cache_shape() {
    let l = Line {
        words: vec![word(1.0, "la", true)],
        opposite_turn: true,
        parent: Some(2),
        ..la(1.0, Some(2.0))
    };
    let text = serde_json::to_string(&l).unwrap();
    assert!(text.contains("\"oppositeTurn\":true") && text.contains("\"joinsNext\":true"));
    assert!(!text.contains("interlude"));
    assert_eq!(serde_json::from_str::<Line>(&text).unwrap(), l);
}

// raw_chunk_words

#[test]
fn raw_chunks_join_only_across_no_whitespace() {
    let raw = [(1.0, "Hel"), (1.2, "lo "), (1.5, " "), (1.6, "world"), (2.0, " again")];
    assert_eq!(
        raw_chunk_words(&raw),
        vec![word(1.0, "Hel", true), word(1.2, "lo", false), word(1.6, "world", false), word(2.0, "again", false)]
    );
}
