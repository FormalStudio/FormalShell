//! The host's RemotePairing (CoreDevice) record for a phone.
//!
//! pymobiledevice3 writes `remote_<UDID>.plist` with `private_key`,
//! `public_key`, `peer_alt_irk` and `remote_unlock_host_key`, and no
//! identifier: it presents `uuid3(NAMESPACE_DNS, hostname)` upper-cased, and
//! pair-verify signs that string, so the case has to match.

use std::path::{Path, PathBuf};

use ed25519_dalek::{SigningKey, VerifyingKey};
use idevice::remote_pairing::RpPairingFile;

pub struct Record {
    pub udid: String,
    pub file: RpPairingFile,
    pub alt_irk: [u8; 16],
}

pub fn dirs() -> Vec<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_default();
    vec![data.join("formalshell/iphone"), data.join("pymobiledevice3")]
}

/// Every record found, newest first.
pub fn all() -> Vec<Record> {
    let mut found: Vec<(std::time::SystemTime, Record)> = Vec::new();
    for dir in dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let Some(udid) = name.strip_prefix("remote_").and_then(|n| n.strip_suffix(".plist")) else {
                continue;
            };
            if found.iter().any(|(_, r)| r.udid == udid) {
                continue;
            }
            match load(&path, udid) {
                Ok(r) => {
                    let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    found.push((mtime, r));
                }
                Err(e) => tracing::warn!("pairing record {}: {e}", path.display()),
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, r)| r).collect()
}

fn load(path: &Path, udid: &str) -> Result<Record, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let dict: plist::Dictionary = plist::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let data = |k: &str| dict.get(k).and_then(|v| v.as_data()).map(|d| d.to_vec());
    let private: [u8; 32] = data("private_key")
        .and_then(|d| d.try_into().ok())
        .ok_or("no private_key")?;
    let public: [u8; 32] = data("public_key").and_then(|d| d.try_into().ok()).ok_or("no public_key")?;
    let alt_irk: [u8; 16] = data("peer_alt_irk")
        .or_else(|| data("alt_irk"))
        .and_then(|d| d.try_into().ok())
        .ok_or("no peer_alt_irk")?;
    let identifier = match dict.get("identifier").or_else(|| dict.get("host_identifier")).and_then(|v| v.as_string()) {
        Some(id) => id.to_string(),
        None => host_identifier(),
    };
    Ok(Record {
        udid: udid.to_string(),
        file: RpPairingFile {
            e_private_key: SigningKey::from_bytes(&private),
            e_public_key: VerifyingKey::from_bytes(&public).map_err(|e| e.to_string())?,
            identifier,
            alt_irk: Some(alt_irk.to_vec()),
        },
        alt_irk,
    })
}

fn host_identifier() -> String {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    uuid::Uuid::new_v3(&uuid::Uuid::NAMESPACE_DNS, host.trim().as_bytes())
        .to_string()
        .to_uppercase()
}
