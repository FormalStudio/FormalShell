//! Pure payload and matrix logic for the network panel's Wi-Fi QR share
//! (omarchy's `bin/omarchy-network-qr` as testable code: its escaping rules,
//! its security-branch choice, and its ASCII-pair collapse, nothing else).
//!
//! The passphrase passes through [`build_payload`]. It stays an argument and a
//! return value: this module never logs it, never keeps it in a static, and
//! never writes it anywhere. The caller holds the returned payload only long
//! enough to write it to qrencode's stdin (never argv, which /proc publishes
//! to every local user).

/// The WIFI: URI reserves four characters. Backslash is doubled first, or the
/// escapes added after it would themselves be re-escaped.
pub(crate) fn escape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | ';' | ',' | ':') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 802.1x networks authenticate against a server, not a shared secret; there
/// is nothing to put in the payload's P: field at all, so the caller renders
/// an honest refusal rather than a QR that cannot work. Both spellings
/// NetworkManager uses for key-mgmt appear here ("wpa-eap", "ieee8021x").
pub fn is_enterprise_key_mgmt(key_mgmt: &str) -> bool {
    let k = key_mgmt.to_lowercase();
    k.contains("eap") || k.contains("ieee8021x")
}

/// What the payload is built from. `key_mgmt` is NetworkManager's word for
/// the profile (`wpa-psk`, `none`, empty for open) and `hidden` is `yes` for
/// a network that does not broadcast its name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    pub ssid: String,
    pub key_mgmt: String,
    pub password: String,
    pub hidden: String,
    pub wep_key: String,
}

/// Why no payload was built. Codes, not display strings: the panel owns the
/// uppercase honest-state text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadError {
    NoSsid,
    Enterprise,
    NoPassword,
}

impl PayloadError {
}

pub fn build_payload(f: &Fields) -> Result<String, PayloadError> {
    if f.ssid.is_empty() {
        return Err(PayloadError::NoSsid);
    }
    if is_enterprise_key_mgmt(&f.key_mgmt) {
        return Err(PayloadError::Enterprise);
    }

    let (security, password) = if !f.key_mgmt.is_empty() && f.key_mgmt != "none" {
        if f.password.is_empty() {
            return Err(PayloadError::NoPassword);
        }
        ("WPA", f.password.as_str())
    } else if !f.wep_key.is_empty() {
        // NetworkManager models WEP as key-mgmt "none" plus a wep-key, so an
        // absent key-mgmt alone does not mean open: encoding this as nopass
        // would produce a QR that silently fails to join.
        ("WEP", f.wep_key.as_str())
    } else {
        // An open network carries no secret, so P: stays empty rather than
        // echoing whatever the psk field happened to hold.
        ("nopass", "")
    };

    let mut payload = format!("WIFI:T:{security};S:{};P:{};", escape_value(&f.ssid), escape_value(password));
    if f.hidden.to_lowercase() == "yes" {
        payload.push_str("H:true;");
    }
    payload.push(';');
    Ok(payload)
}

/// `qrencode --type ASCII` writes every module as TWO characters ("##" dark,
/// "  " light) so the code stays square in a terminal's 2:1 character cell.
/// Collapse each pair back to one "0"/"1" and hand back the row strings the
/// caller can render as real square rectangles.
///
/// A QR symbol is square and `--margin` applies the quiet zone on all four
/// sides, so the collapsed matrix must have as many columns as rows. Anything
/// else, empty stdout, a truncated read, an all-space quiet-zone row trimmed
/// away by something in the pipe, returns an empty matrix, so the caller shows
/// an honest error instead of a partial or padded code.
pub fn parse_matrix(ascii: &str) -> Vec<String> {
    let mut lines: Vec<&str> = ascii.split('\n').collect();
    while lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let hash = u16::from(b'#');
    let mut rows = Vec::new();
    for line in &lines {
        let units: Vec<u16> = line.encode_utf16().collect();
        if !units.len().is_multiple_of(2) {
            return Vec::new();
        }
        let row: String = units.chunks(2).map(|p| if p[0] == hash || p[1] == hash { '1' } else { '0' }).collect();
        if row.len() != lines.len() {
            return Vec::new();
        }
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(ssid: &str, key_mgmt: &str, password: &str, hidden: &str, wep_key: &str) -> Fields {
        Fields { ssid: ssid.into(), key_mgmt: key_mgmt.into(), password: password.into(), hidden: hidden.into(), wep_key: wep_key.into() }
    }

    // escape_value

    #[test]
    fn escape_backslash_is_doubled() {
        assert_eq!(escape_value("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_semicolon() {
        assert_eq!(escape_value("a;b"), "a\\;b");
    }

    #[test]
    fn escape_comma() {
        assert_eq!(escape_value("a,b"), "a\\,b");
    }

    #[test]
    fn escape_colon() {
        assert_eq!(escape_value("a:b"), "a\\:b");
    }

    // Backslash first, or each escape added afterwards would itself be
    // re-escaped by the backslash pass.
    #[test]
    fn escape_applies_backslash_before_the_others() {
        assert_eq!(escape_value("\\;"), "\\\\\\;");
    }

    #[test]
    fn escape_leaves_ordinary_text_untouched() {
        assert_eq!(escape_value("FORMALTEST"), "FORMALTEST");
    }

    #[test]
    fn escape_empty_value() {
        assert_eq!(escape_value(""), "");
    }

    // build_payload: security branches

    #[test]
    fn payload_wpa_branch() {
        let r = build_payload(&fields("FORMALTEST", "wpa-psk", "formaltest-psk", "", ""));
        assert_eq!(r.unwrap(), "WIFI:T:WPA;S:FORMALTEST;P:formaltest-psk;;");
    }

    #[test]
    fn payload_nopass_branch_for_key_mgmt_none() {
        assert_eq!(build_payload(&fields("GUEST", "none", "", "", "")).unwrap(), "WIFI:T:nopass;S:GUEST;P:;;");
    }

    #[test]
    fn payload_nopass_branch_for_absent_key_mgmt() {
        assert_eq!(build_payload(&fields("GUEST", "", "", "", "")).unwrap(), "WIFI:T:nopass;S:GUEST;P:;;");
    }

    // An open network has no secret to share, so a psk that somehow came back
    // alongside key-mgmt "none" is dropped rather than published.
    #[test]
    fn payload_nopass_never_echoes_a_stray_psk() {
        assert_eq!(build_payload(&fields("GUEST", "none", "leftover", "", "")).unwrap(), "WIFI:T:nopass;S:GUEST;P:;;");
    }

    // NetworkManager models WEP as key-mgmt "none" plus a wep-key0.
    #[test]
    fn payload_wep_branch_from_wep_key_with_no_key_mgmt() {
        assert_eq!(build_payload(&fields("OLDNET", "none", "", "", "abcde")).unwrap(), "WIFI:T:WEP;S:OLDNET;P:abcde;;");
    }

    #[test]
    fn payload_wpa_wins_over_a_wep_key_when_key_mgmt_is_set() {
        assert_eq!(build_payload(&fields("NET", "wpa-psk", "pw", "", "abcde")).unwrap(), "WIFI:T:WPA;S:NET;P:pw;;");
    }

    // build_payload: hidden

    #[test]
    fn payload_hidden_flag() {
        assert_eq!(build_payload(&fields("NET", "wpa-psk", "pw", "yes", "")).unwrap(), "WIFI:T:WPA;S:NET;P:pw;H:true;;");
    }

    #[test]
    fn payload_hidden_no_omits_the_flag() {
        assert_eq!(build_payload(&fields("NET", "wpa-psk", "pw", "no", "")).unwrap(), "WIFI:T:WPA;S:NET;P:pw;;");
    }

    // build_payload: escaping reaches both fields

    #[test]
    fn payload_escapes_ssid_and_password() {
        let r = build_payload(&fields("a;b,c:d\\e", "wpa-psk", "p:q;r", "", ""));
        assert_eq!(r.unwrap(), "WIFI:T:WPA;S:a\\;b\\,c\\:d\\\\e;P:p\\:q\\;r;;");
    }

    // build_payload: refusals

    #[test]
    fn payload_refuses_enterprise_ieee8021x() {
        assert_eq!(build_payload(&fields("CORP", "ieee8021x", "pw", "", "")), Err(PayloadError::Enterprise));
    }

    #[test]
    fn payload_refuses_empty_fields() {
        assert_eq!(build_payload(&Fields::default()), Err(PayloadError::NoSsid));
    }

    // parse_matrix

    #[test]
    fn parse_matrix_collapses_two_characters_per_module() {
        let m = parse_matrix("##  ##\n  ##  \n######\n");
        assert_eq!(m, ["101", "010", "111"]);
    }

    #[test]
    fn parse_matrix_keeps_an_all_light_quiet_zone_row() {
        let m = parse_matrix("      \n##  ##\n      \n");
        assert_eq!(m, ["000", "101", "000"]);
    }

    #[test]
    fn parse_matrix_without_a_trailing_newline() {
        assert_eq!(parse_matrix("####\n  ##"), ["11", "01"]);
    }

    #[test]
    fn parse_matrix_empty_input() {
        assert_eq!(parse_matrix("").len(), 0);
    }

    #[test]
    fn parse_matrix_odd_length_row_is_not_a_module_grid() {
        assert_eq!(parse_matrix("#####\n#####\n").len(), 0);
    }

    #[test]
    fn parse_matrix_non_square_is_rejected() {
        assert_eq!(parse_matrix("##  ##\n  ##  \n").len(), 0);
    }

    #[test]
    fn parse_matrix_ragged_rows_are_rejected() {
        assert_eq!(parse_matrix("##  ##\n  ##\n######\n").len(), 0);
    }

    #[test]
    fn parse_matrix_arbitrary_text_is_rejected() {
        assert_eq!(parse_matrix("qrencode: failed to encode\n").len(), 0);
    }
}
