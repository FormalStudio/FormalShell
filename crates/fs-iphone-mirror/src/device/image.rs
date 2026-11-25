//! The developer disk image the display and HID services live in. It stays
//! mounted until the phone restarts; a restarted phone gets the cached
//! personalized image back (personalized through Apple's TSS), which needs
//! the phone unlocked.

use std::path::PathBuf;

use idevice::RsdService;
use idevice::mobile_image_mounter::ImageMounter;

use super::tunnel::Tunnel;

pub async fn mounted(tunnel: &mut Tunnel) -> Result<bool, String> {
    let mut mounter = ImageMounter::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
        .await
        .map_err(|e| format!("image mounter: {e:?}"))?;
    match mounter.lookup_image("Personalized").await {
        Ok(_) => Ok(true),
        Err(idevice::IdeviceError::NotFound) => Ok(false),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// The cached image: ours, or the one pymobiledevice3 downloaded.
fn cache() -> Option<PathBuf> {
    super::pairing::dirs()
        .into_iter()
        .map(|d| if d.ends_with("pymobiledevice3") { d.join("Xcode_iOS_DDI_Personalized") } else { d.join("ddi") })
        .find(|d| d.join("Image.dmg").is_file() && d.join("BuildManifest.plist").is_file() && d.join("Image.trustcache").is_file())
}

pub async fn mount(tunnel: &mut Tunnel) -> Result<(), String> {
    let dir = cache().ok_or("no cached developer disk image")?;
    let image = tokio::fs::read(dir.join("Image.dmg")).await.map_err(|e| e.to_string())?;
    let trust = tokio::fs::read(dir.join("Image.trustcache")).await.map_err(|e| e.to_string())?;
    let manifest = tokio::fs::read(dir.join("BuildManifest.plist")).await.map_err(|e| e.to_string())?;
    let ecid = tunnel
        .rsd
        .properties
        .get("UniqueChipID")
        .and_then(|v| v.as_unsigned_integer().or(v.as_signed_integer().map(|i| i as u64)))
        .ok_or("no UniqueChipID")?;
    let mut mounter = ImageMounter::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
        .await
        .map_err(|e| format!("image mounter: {e:?}"))?;
    mounter
        .mount_personalized_rsd(&mut tunnel.adapter, &mut tunnel.rsd, image, trust, &manifest, None, ecid)
        .await
        .map_err(|e| format!("mount: {e:?}"))
}
