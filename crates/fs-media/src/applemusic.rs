//! Apple Music animated-cover glue, ported with attribution from
//! AvengeMedia/DankMaterialShell PR #2918 (MIT).
//!
//! URL construction, every response-parsing step of the undocumented chain
//! (iTunes artist search matched by exact artist name, that artist's album
//! list matched by exact album name, a scraped web-player token, amp-api
//! `editorialVideo`, HLS master and rendition playlists, the one
//! progressive-mp4 byterange) and the cache-key and prune decisions. No
//! network, disk or clock here: every parse function takes the raw process
//! exit code beside the output text, so a curl failure and a 200 with a
//! garbage body get distinct errors.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use fs_js::{encode_uri_component, parse_digits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    HttpError,
    MalformedJson,
    MissingFields,
}

impl ParseError {
    pub fn as_str(self) -> &'static str {
        match self {
            ParseError::HttpError => "http_error",
            ParseError::MalformedJson => "malformed_json",
            ParseError::MissingFields => "missing_fields",
        }
    }
}

static NON_ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static EDGE_DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-+|-+$").unwrap());
static EDITION_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[(\[][^()\[\]]*[)\]]\s*$").unwrap());
static SINGLE_EP_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*-\s*(?:single|ep)\s*$").unwrap());
static ASSET_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/assets/index~[A-Za-z0-9]+\.js").unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""(eyJ[A-Za-z0-9._-]+)""#).unwrap());
static RESOLUTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"RESOLUTION=([0-9]+)x").unwrap());
static AVG_BANDWIDTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AVERAGE-BANDWIDTH=([0-9]+)").unwrap());
static EXT_X_MAP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#EXT-X-MAP:URI="([^"]+)""#).unwrap());

pub fn cache_key(artist: &str, album: &str) -> String {
    let raw = format!("{artist} {album}").to_lowercase();
    let dashed = NON_ALNUM.replace_all(&raw, "-");
    EDGE_DASHES.replace_all(&dashed, "").into_owned()
}

/// Loosely matches iTunes' own album-title conventions: case, punctuation and
/// whitespace are folded, and a trailing "- Single"/"- EP" marker or a
/// trailing "(Deluxe)"/"[Remastered]" tag is stripped, in either order and
/// repeatedly. Used to compare both sides of a match, never to build a URL.
pub(crate) fn normalize_name(name: &str) -> String {
    let mut s = name.trim().to_string();
    loop {
        let without_tag = EDITION_TAG.replace(&s, "").trim().to_string();
        if without_tag != s {
            s = without_tag;
            continue;
        }
        let without_suffix = SINGLE_EP_SUFFIX.replace(&s, "").trim().to_string();
        if without_suffix != s {
            s = without_suffix;
            continue;
        }
        break;
    }
    NON_ALNUM
        .replace_all(&s.to_lowercase(), " ")
        .trim()
        .to_string()
}

pub fn artist_search_url(artist: &str) -> Option<String> {
    if artist.is_empty() {
        return None;
    }
    Some(format!(
        "https://itunes.apple.com/search?media=music&entity=musicArtist&limit=5&term={}",
        encode_uri_component(artist)
    ))
}

fn parse_results(exit_code: i32, output: &str) -> Result<Vec<Value>, ParseError> {
    if exit_code != 0 {
        return Err(ParseError::HttpError);
    }
    let data: Value = serde_json::from_str(output).map_err(|_| ParseError::MalformedJson)?;
    Ok(match data.get("results") {
        Some(Value::Array(results)) => results.clone(),
        _ => Vec::new(),
    })
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// iTunes' artist search is a name search, so it returns every artist sharing
/// the term. A hit is the result whose normalised artistName equals the
/// player's normalised artist exactly; anything else is a miss (`None`),
/// never a guess.
pub fn parse_artist_search_result(
    exit_code: i32,
    output: &str,
    artist: &str,
) -> Result<Option<i64>, ParseError> {
    let results = parse_results(exit_code, output)?;
    let target = normalize_name(artist);
    for r in &results {
        if normalize_name(str_field(r, "artistName")) == target {
            return Ok(r.get("artistId").and_then(Value::as_i64));
        }
    }
    Ok(None)
}

pub fn artist_albums_url(artist_id: impl std::fmt::Display) -> String {
    format!("https://itunes.apple.com/lookup?id={artist_id}&entity=album&limit=200")
}

/// The artist lookup echoes the artist itself ahead of every album
/// (wrapperType "collection"), so only collections are considered. An exact
/// raw collectionName match wins over a normalised one, so a plain "IGOR"
/// picks the plain edition over a "(Deluxe)" reissue sitting earlier in the
/// list. No match is a miss (`None`).
pub fn parse_artist_albums_result(
    exit_code: i32,
    output: &str,
    album: &str,
) -> Result<Option<i64>, ParseError> {
    let results = parse_results(exit_code, output)?;
    let target = normalize_name(album);
    let mut normalized_match: Option<Option<i64>> = None;
    for r in &results {
        if str_field(r, "wrapperType") != "collection" {
            continue;
        }
        let id = r.get("collectionId").and_then(Value::as_i64);
        let name = str_field(r, "collectionName");
        if name == album {
            return Ok(id);
        }
        if normalized_match.is_none() && normalize_name(name) == target {
            normalized_match = Some(id);
        }
    }
    Ok(normalized_match.flatten())
}

pub fn album_page_url(collection_id: impl std::fmt::Display) -> String {
    format!("https://music.apple.com/us/album/{collection_id}")
}

/// The anonymous web-player JWT sits in the main JS bundle referenced by any
/// album page: the asset path first, then the token out of that bundle.
pub fn extract_asset_path(html: &str) -> Option<String> {
    ASSET_PATH.find(html).map(|m| m.as_str().to_string())
}

pub fn extract_token(js_text: &str) -> Option<String> {
    TOKEN.captures(js_text).map(|m| m[1].to_string())
}

pub fn amp_api_url(collection_id: impl std::fmt::Display) -> String {
    format!(
        "https://amp-api.music.apple.com/v1/catalog/us/albums/{collection_id}?extend=editorialVideo"
    )
}

fn truthy(v: Option<&Value>) -> Option<&Value> {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => None,
        Some(v) => Some(v),
    }
}

/// A hit is a non-empty HLS master-playlist URL; a miss (no editorialVideo, or
/// one with neither known video field) still parses ok with an empty URL so
/// the caller caches "no animated art" rather than retrying every play.
pub fn parse_editorial_video(exit_code: i32, output: &str) -> Result<String, ParseError> {
    if exit_code != 0 {
        return Err(ParseError::HttpError);
    }
    let data: Value = serde_json::from_str(output).map_err(|_| ParseError::MalformedJson)?;
    let entry = match data.get("data") {
        Some(Value::Array(a)) => a.first(),
        _ => None,
    };
    let attributes =
        truthy(entry.and_then(|e| e.get("attributes"))).ok_or(ParseError::MissingFields)?;
    let Some(ev) = truthy(attributes.get("editorialVideo")) else {
        return Ok(String::new());
    };
    let video = |key: &str| -> Option<String> {
        let v = truthy(ev.get(key))?;
        match truthy(v.get("video"))? {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    };
    Ok(video("motionDetailSquare")
        .or_else(|| video("motionSquareVideo1x1"))
        .unwrap_or_default())
}

pub fn resolve_url(maybe_relative: &str, base_url: &str) -> String {
    if maybe_relative.starts_with("http") {
        return maybe_relative.to_string();
    }
    let dir_end = base_url.rfind('/').map(|i| i + 1).unwrap_or(0);
    format!("{}{maybe_relative}", &base_url[..dir_end])
}

/// Highest-bandwidth avc1 rendition at or below 768px; hvc1 is skipped for
/// decoder compatibility. Returns the (possibly relative) variant playlist
/// path, or None when the master has no eligible stream.
pub fn pick_variant(master_playlist_text: &str) -> Option<String> {
    let lines: Vec<&str> = master_playlist_text.split('\n').collect();
    let mut best: Option<String> = None;
    let mut best_bandwidth: i128 = -1;
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with("#EXT-X-STREAM-INF:") || !line.contains("avc1") {
            continue;
        }
        let Some(resolution) = RESOLUTION.captures(line) else {
            continue;
        };
        if parse_digits(&resolution[1]) > 768 {
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() && (lines[j].starts_with('#') || lines[j].trim().is_empty()) {
            j += 1;
        }
        if j >= lines.len() {
            continue;
        }
        let bandwidth = AVG_BANDWIDTH
            .captures(line)
            .map(|b| parse_digits(&b[1]) as i128)
            .unwrap_or(0);
        if bandwidth > best_bandwidth {
            best_bandwidth = bandwidth;
            best = Some(lines[j].trim().to_string());
        }
    }
    best
}

/// The rendition playlist is BYTERANGE segments over one progressive mp4 named
/// by EXT-X-MAP; that mp4 is the actual download target.
pub fn extract_mp4_url(rendition_playlist_text: &str, variant_url: &str) -> Option<String> {
    EXT_X_MAP
        .captures(rendition_playlist_text)
        .map(|m| resolve_url(&m[1], variant_url))
}

/// A file untouched for more than `max_age_days` is stale. Exactly that old is
/// not yet stale, matching `find -mtime +N`'s inclusive boundary.
pub fn is_stale(mtime_ms: f64, now_ms: f64, max_age_days: f64) -> bool {
    if !mtime_ms.is_finite() {
        return false;
    }
    let max_age_ms = max_age_days * 24.0 * 60.0 * 60.0 * 1000.0;
    (now_ms - mtime_ms) > max_age_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DAY_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
    // Date.UTC(2026, 6, 28)
    const NOW_2026_07_28_MS: f64 = 1_785_196_800_000.0;

    #[test]
    fn cache_key_lowercases_and_dashes() {
        assert_eq!(
            cache_key("The Beatles", "Abbey Road"),
            "the-beatles-abbey-road"
        );
    }

    #[test]
    fn cache_key_strips_punctuation() {
        assert_eq!(cache_key("Sigur Rós", "( )"), "sigur-r-s");
    }

    #[test]
    fn normalize_name_lowercases_and_folds_punctuation() {
        assert_eq!(normalize_name("Tyler, The Creator"), "tyler the creator");
    }

    #[test]
    fn normalize_name_strips_trailing_single_suffix() {
        assert_eq!(normalize_name("Yonkers - Single"), "yonkers");
    }

    #[test]
    fn normalize_name_strips_trailing_ep_suffix() {
        assert_eq!(normalize_name("Cherry Bomb - EP"), "cherry bomb");
    }

    #[test]
    fn normalize_name_strips_trailing_edition_tag() {
        assert_eq!(normalize_name("IGOR (Deluxe)"), "igor");
        assert_eq!(normalize_name("IGOR [Remastered]"), "igor");
    }

    #[test]
    fn normalize_name_strips_tag_and_suffix_in_either_order() {
        assert_eq!(normalize_name("Anti (Deluxe) - Single"), "anti");
        assert_eq!(normalize_name("Anti - Single (Deluxe)"), "anti");
    }

    #[test]
    fn artist_search_url_targets_itunes_artist_search() {
        let url = artist_search_url("Tyler, The Creator").unwrap();
        assert!(url.starts_with("https://itunes.apple.com/search?"));
        assert!(url.contains("media=music"));
        assert!(url.contains("entity=musicArtist"));
        assert!(url.contains("term=Tyler%2C%20The%20Creator"));
    }

    #[test]
    fn artist_search_url_none_without_artist() {
        assert_eq!(artist_search_url(""), None);
    }

    fn tyler_artist_search_body() -> String {
        json!({ "results": [
            { "wrapperType": "artist", "artistName": "Tyla", "artistId": 1508583854 },
            { "wrapperType": "artist", "artistName": "Not Tyler, The Creator", "artistId": 999888777 },
            { "wrapperType": "artist", "artistName": "Tyler, The Creator", "artistId": 420368335 }
        ] })
        .to_string()
    }

    #[test]
    fn parse_artist_search_result_picks_exact_name_match_not_first_hit() {
        let r = parse_artist_search_result(0, &tyler_artist_search_body(), "Tyler, The Creator");
        assert_eq!(r, Ok(Some(420368335)));
    }

    #[test]
    fn parse_artist_search_result_rejects_similarly_named_artist() {
        let body = json!({ "results": [
            { "wrapperType": "artist", "artistName": "Not Tyler, The Creator", "artistId": 999888777 }
        ] })
        .to_string();
        assert_eq!(
            parse_artist_search_result(0, &body, "Tyler, The Creator"),
            Ok(None)
        );
    }

    #[test]
    fn parse_artist_search_result_miss_no_results() {
        let body = json!({ "results": [] }).to_string();
        assert_eq!(parse_artist_search_result(0, &body, "Air"), Ok(None));
    }

    #[test]
    fn parse_artist_search_result_malformed_json() {
        let r = parse_artist_search_result(0, "not json{{{", "Air");
        assert_eq!(r, Err(ParseError::MalformedJson));
        assert_eq!(r.unwrap_err().as_str(), "malformed_json");
    }

    #[test]
    fn parse_artist_search_result_http_error_on_nonzero_exit() {
        let r = parse_artist_search_result(1, "", "Air");
        assert_eq!(r, Err(ParseError::HttpError));
        assert_eq!(r.unwrap_err().as_str(), "http_error");
    }

    #[test]
    fn artist_albums_url_targets_itunes_album_lookup() {
        assert_eq!(
            artist_albums_url(420368335),
            "https://itunes.apple.com/lookup?id=420368335&entity=album&limit=200"
        );
    }

    fn tyler_albums_body() -> String {
        json!({ "results": [
            { "wrapperType": "artist", "artistName": "Tyler, The Creator", "artistId": 420368335 },
            { "wrapperType": "collection", "collectionName": "Flower Boy", "collectionId": 1272757506 },
            { "wrapperType": "collection", "collectionName": "IGOR (Deluxe)", "collectionId": 1465524276 },
            { "wrapperType": "collection", "collectionName": "IGOR", "collectionId": 1461407974 },
            { "wrapperType": "collection", "collectionName": "CALL ME IF YOU GET LOST", "collectionId": 1585047605 }
        ] })
        .to_string()
    }

    #[test]
    fn parse_artist_albums_result_picks_exact_match_over_edition_tag() {
        assert_eq!(
            parse_artist_albums_result(0, &tyler_albums_body(), "IGOR"),
            Ok(Some(1461407974))
        );
    }

    #[test]
    fn parse_artist_albums_result_falls_back_to_normalised_match() {
        let body = json!({ "results": [
            { "wrapperType": "collection", "collectionName": "IGOR (Deluxe)", "collectionId": 1465524276 }
        ] })
        .to_string();
        assert_eq!(
            parse_artist_albums_result(0, &body, "IGOR"),
            Ok(Some(1465524276))
        );
    }

    #[test]
    fn parse_artist_albums_result_matches_single_suffix() {
        let body = json!({ "results": [
            { "wrapperType": "collection", "collectionName": "Yonkers - Single", "collectionId": 424415818 }
        ] })
        .to_string();
        assert_eq!(
            parse_artist_albums_result(0, &body, "Yonkers"),
            Ok(Some(424415818))
        );
    }

    #[test]
    fn parse_artist_albums_result_ignores_artist_entity() {
        let body = json!({ "results": [
            { "wrapperType": "artist", "artistName": "IGOR", "artistId": 1 }
        ] })
        .to_string();
        assert_eq!(parse_artist_albums_result(0, &body, "IGOR"), Ok(None));
    }

    #[test]
    fn parse_artist_albums_result_miss_no_matching_album() {
        assert_eq!(
            parse_artist_albums_result(0, &tyler_albums_body(), "Scum Fuck Flower Boy"),
            Ok(None)
        );
    }

    #[test]
    fn parse_artist_albums_result_malformed_json() {
        assert_eq!(
            parse_artist_albums_result(0, "not json{{{", "IGOR"),
            Err(ParseError::MalformedJson)
        );
    }

    #[test]
    fn parse_artist_albums_result_http_error_on_nonzero_exit() {
        assert_eq!(
            parse_artist_albums_result(1, "", "IGOR"),
            Err(ParseError::HttpError)
        );
    }

    #[test]
    fn album_page_url_is_us_storefront() {
        assert_eq!(
            album_page_url(1440833449),
            "https://music.apple.com/us/album/1440833449"
        );
    }

    #[test]
    fn extract_asset_path_finds_hashed_bundle() {
        let html = r#"<script src="/assets/index~a1B2c3.js" defer></script>"#;
        assert_eq!(
            extract_asset_path(html).as_deref(),
            Some("/assets/index~a1B2c3.js")
        );
    }

    #[test]
    fn extract_asset_path_none_when_absent() {
        assert_eq!(extract_asset_path("<html></html>"), None);
    }

    #[test]
    fn extract_token_finds_jwt_looking_string() {
        let js = r#"const t="eyJhbGciOiJFUzI1NiJ9.abc-def_123.sig";"#;
        assert_eq!(
            extract_token(js).as_deref(),
            Some("eyJhbGciOiJFUzI1NiJ9.abc-def_123.sig")
        );
    }

    #[test]
    fn extract_token_none_when_absent() {
        assert_eq!(extract_token("const t=1;"), None);
    }

    #[test]
    fn amp_api_url_requests_editorial_video_extension() {
        assert_eq!(
            amp_api_url(1440833449),
            "https://amp-api.music.apple.com/v1/catalog/us/albums/1440833449?extend=editorialVideo"
        );
    }

    #[test]
    fn parse_editorial_video_hit_prefers_motion_detail_square() {
        let body = json!({ "data": [{ "attributes": { "editorialVideo": {
            "motionDetailSquare": { "video": "https://a.example/detail.m3u8" },
            "motionSquareVideo1x1": { "video": "https://a.example/square.m3u8" }
        } } }] })
        .to_string();
        assert_eq!(
            parse_editorial_video(0, &body),
            Ok("https://a.example/detail.m3u8".into())
        );
    }

    #[test]
    fn parse_editorial_video_hit_falls_back_to_square_1x1() {
        let body = json!({ "data": [{ "attributes": { "editorialVideo": {
            "motionSquareVideo1x1": { "video": "https://a.example/square.m3u8" }
        } } }] })
        .to_string();
        assert_eq!(
            parse_editorial_video(0, &body),
            Ok("https://a.example/square.m3u8".into())
        );
    }

    #[test]
    fn parse_editorial_video_miss_no_editorial_video_field() {
        let body = json!({ "data": [{ "attributes": {} }] }).to_string();
        assert_eq!(parse_editorial_video(0, &body), Ok(String::new()));
    }

    #[test]
    fn parse_editorial_video_miss_empty_video_fields() {
        let body = json!({ "data": [{ "attributes": { "editorialVideo": {} } }] }).to_string();
        assert_eq!(parse_editorial_video(0, &body), Ok(String::new()));
    }

    #[test]
    fn parse_editorial_video_malformed_json() {
        assert_eq!(
            parse_editorial_video(0, "not json{{{"),
            Err(ParseError::MalformedJson)
        );
    }

    #[test]
    fn parse_editorial_video_missing_data_array() {
        assert_eq!(
            parse_editorial_video(0, "{}"),
            Err(ParseError::MissingFields)
        );
    }

    #[test]
    fn parse_editorial_video_http_error_on_nonzero_exit() {
        assert_eq!(parse_editorial_video(1, ""), Err(ParseError::HttpError));
    }

    #[test]
    fn resolve_url_passes_through_absolute() {
        assert_eq!(
            resolve_url(
                "https://a.example/x.mp4",
                "https://a.example/base/master.m3u8"
            ),
            "https://a.example/x.mp4"
        );
    }

    #[test]
    fn resolve_url_joins_relative_to_base_directory() {
        assert_eq!(
            resolve_url("hi/rendition.m3u8", "https://a.example/base/master.m3u8"),
            "https://a.example/base/hi/rendition.m3u8"
        );
    }

    #[test]
    fn pick_variant_selects_highest_bandwidth_avc1_under_768() {
        let master = [
            "#EXTM3U",
            "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=500000,RESOLUTION=320x320,CODECS=\"avc1.640015\"",
            "low/index.m3u8",
            "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=2000000,RESOLUTION=640x640,CODECS=\"avc1.640028\"",
            "high/index.m3u8",
        ]
        .join("\n");
        assert_eq!(pick_variant(&master).as_deref(), Some("high/index.m3u8"));
    }

    #[test]
    fn pick_variant_skips_resolutions_above_768() {
        let master = [
            "#EXTM3U",
            "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=500000,RESOLUTION=320x320,CODECS=\"avc1.640015\"",
            "low/index.m3u8",
            "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=9000000,RESOLUTION=1080x1080,CODECS=\"avc1.640028\"",
            "huge/index.m3u8",
        ]
        .join("\n");
        assert_eq!(pick_variant(&master).as_deref(), Some("low/index.m3u8"));
    }

    #[test]
    fn pick_variant_skips_non_avc1_codecs() {
        let master = [
            "#EXTM3U",
            "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=2000000,RESOLUTION=640x640,CODECS=\"hvc1.1.6.L93.90\"",
            "hevc/index.m3u8",
        ]
        .join("\n");
        assert_eq!(pick_variant(&master), None);
    }

    #[test]
    fn pick_variant_none_on_empty_master() {
        assert_eq!(pick_variant(""), None);
    }

    #[test]
    fn extract_mp4_url_resolves_relative_map_uri() {
        let rendition = [
            "#EXTM3U",
            "#EXT-X-MAP:URI=\"segment0.mp4\",BYTERANGE=\"800@0\"",
            "#EXT-X-BYTERANGE:200000@800",
            "seg.mp4",
        ]
        .join("\n");
        assert_eq!(
            extract_mp4_url(&rendition, "https://a.example/base/hi/index.m3u8").as_deref(),
            Some("https://a.example/base/hi/segment0.mp4")
        );
    }

    #[test]
    fn extract_mp4_url_none_without_map_tag() {
        assert_eq!(
            extract_mp4_url("#EXTM3U\nseg.mp4", "https://a.example/base/hi/index.m3u8"),
            None
        );
    }

    #[test]
    fn is_stale_false_at_exact_boundary() {
        let now = NOW_2026_07_28_MS;
        assert!(!is_stale(now - 30.0 * DAY_MS, now, 30.0));
    }

    #[test]
    fn is_stale_true_one_ms_past_boundary() {
        let now = NOW_2026_07_28_MS;
        assert!(is_stale(now - 30.0 * DAY_MS - 1.0, now, 30.0));
    }

    #[test]
    fn is_stale_false_for_fresh_file() {
        let now = NOW_2026_07_28_MS;
        assert!(!is_stale(now - 60000.0, now, 30.0));
    }

    #[test]
    fn is_stale_false_for_non_numeric_mtime() {
        // undefined in JS; the caller's NaN here.
        assert!(!is_stale(f64::NAN, NOW_2026_07_28_MS, 30.0));
    }

}
