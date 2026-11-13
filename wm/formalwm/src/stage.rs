//! How an output is put together when the plain stack will not do: the dim
//! under a shown overlay, the overview's backdrop and workspace panels. An
//! output with neither keeps the default stacking, which is what lets a
//! fullscreen client go straight to the plane.

use std::collections::BTreeMap;

use formalwm_core::{color::Rgba, model::Layer, overview::Lens, scene::Visual};
use shojiwm_rs::prelude::*;

use crate::engine::{Engine, Signals};

/// Hyprland's `dim_special`.
const DIM: f64 = 0.6;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    pub outputs: BTreeMap<String, Draw>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    /// The workspace's windows, the dim, then the overlay's.
    Dim {
        under: Vec<String>,
        over: Vec<String>,
        alpha: f64,
    },
    /// A wash over the wallpaper and one panel behind each workspace.
    Overview {
        wash: [f64; 4],
        panel: [f64; 4],
        panels: Vec<[f64; 4]>,
    },
}

pub fn setup(signals: Signals) {
    COMPOSITOR.rendering.composition(move |output| {
        let draw = signals
            .stage
            .with(|stage| stage.outputs.get(&output.name).cloned());
        let below = Layers::new([LayerName::Background, LayerName::Bottom]);
        let above = Layers::new([LayerName::Top, LayerName::Overlay]);
        match draw {
            None => OutputStack::default_stacking(),
            Some(Draw::Dim { under, over, alpha }) => OutputStack::new()
                .child(below)
                .child(Windows::only(under))
                .child(Solid::new([0.0, 0.0, 0.0, alpha]))
                .child(Windows::only(over))
                .child(above)
                .child(LayerPopups),
            Some(Draw::Overview {
                wash,
                panel,
                panels,
            }) => {
                let mut stack = OutputStack::new().child(below).child(Solid::new(wash));
                for [x, y, w, h] in panels {
                    stack = stack.child(Solid::new(panel).rect(CompositionRect::new(x, y, w, h)));
                }
                stack.child(Windows::all()).child(above).child(LayerPopups)
            }
        }
    });
}

pub fn build(engine: &Engine, frame: &[Visual]) -> Stage {
    let mut stage = Stage::default();
    let background = Rgba::resolve("background", &engine.theme.colors).unwrap_or(Rgba::BLACK);
    let [r, g, b, _] = background.0;
    for output in &engine.model.outputs {
        let name = &output.geo.name;
        let area = output.geo.rect;
        let progress = engine.overview.progress.on(name).clamp(0.0, 1.0);
        if progress > 0.0 {
            let lens = Lens::new(area.w, area.h, progress);
            let at = engine.scene.stack_of(name);
            let panels = (0..engine.model.stack(name).len())
                .map(|index| lens.panel(index as f64 - at))
                .filter(|panel| panel.bottom() > 0.0 && panel.y < area.h)
                .map(|panel| [panel.x, panel.y, panel.w, panel.h])
                .collect();
            stage.outputs.insert(
                name.clone(),
                Draw::Overview {
                    wash: [0.0, 0.0, 0.0, 0.45 * progress],
                    panel: [r, g, b, 0.5 * progress],
                    panels,
                },
            );
            continue;
        }
        let on_output = |visual: &&Visual| &visual.output == name && visual.visible;
        let over: Vec<&Visual> = frame
            .iter()
            .filter(on_output)
            .filter(|visual| visual.layer == Layer::Overlay)
            .collect();
        let Some(first) = over.first() else {
            continue;
        };
        // The overlay drops in from one output height up; the dim comes with it.
        let rest = engine
            .model
            .window(&first.id)
            .map_or(first.rect.y, |window| area.y + window.float_rect.y);
        let shown = 1.0 - ((rest - first.rect.y) / area.h.max(1.0)).clamp(0.0, 1.0);
        stage.outputs.insert(
            name.clone(),
            Draw::Dim {
                under: frame
                    .iter()
                    .filter(on_output)
                    .filter(|visual| visual.layer != Layer::Overlay)
                    .map(|visual| visual.id.clone())
                    .collect(),
                over: over.iter().map(|visual| visual.id.clone()).collect(),
                alpha: DIM * shown,
            },
        );
    }
    stage
}
