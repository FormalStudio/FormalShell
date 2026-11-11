//! Blur behind the layers the settings name and behind translucent
//! windows, as `hyprland.lua`'s layer rules and `decoration.blur` had it.

use std::{path::PathBuf, rc::Rc};

use formalwm_core::settings::Settings;
use shojiwm_rs::prelude::*;

use crate::windows;

/// Hyprland's `blur.size` and `blur.passes`.
const RADIUS: i32 = 8;
const PASSES: i32 = 2;
const MASK: &str = "./shaders/blur-mask.frag";

/// Shaders are read from files under the asset root, so the one the binary
/// carries is written out where the session can find it.
pub fn install_assets() -> PathBuf {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("formalwm");
    let shaders = root.join("shaders");
    if let Err(error) = std::fs::create_dir_all(&shaders).and_then(|()| {
        std::fs::write(
            shaders.join("blur-mask.frag"),
            include_str!("../assets/shaders/blur-mask.frag"),
        )
    }) {
        eprintln!("formalwm: the blur shader could not be written: {error}");
    }
    root
}

fn blur() -> Effect {
    Effect::new(backdrop_source())
        .capture_padding(24)
        .invalidate(Invalidate::on_source_damage_box(8))
        .preserve_alpha()
        .stage(dual_kawase_blur(RADIUS, PASSES))
}

/// The blur kept to where the surface itself is more opaque than
/// `threshold`, so a rounded or soft-edged surface blurs to its own shape.
fn masked(mask: Source, threshold: f64) -> Effect {
    blur().stage(
        shader_stage(MASK)
            .texture("layer_mask", mask)
            .uniform("opacity_threshold", threshold as f32)
            .uniform("mask_feather", 0.04_f32),
    )
}

/// Registers the effects for these settings; called again when the
/// settings or the theme's blur change.
pub fn install(settings: Rc<Settings>, enabled: bool) {
    COMPOSITOR.effect.layer(move |layer| {
        let rule = layer
            .namespace
            .as_deref()
            .and_then(|namespace| settings.layer_rule(namespace));
        match rule {
            Some(rule) if enabled && rule.blur => {
                SurfaceEffects::behind(masked(layer_source(), rule.alpha_threshold))
            }
            _ => SurfaceEffects::none(),
        }
    });
    COMPOSITOR.effect.window(|window| {
        if window.state(&windows::BLUR).get() {
            SurfaceEffects::behind(masked(window_source(), 0.25))
        } else {
            SurfaceEffects::none()
        }
    });
    COMPOSITOR
        .effect
        .background_with(move || enabled.then(blur));
}
