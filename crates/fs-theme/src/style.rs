//! The resolver behind `Theme.box()` (`shell/Theme/style.js`): a theme is
//! one table of box descriptions keyed by role, and this turns an entry plus
//! a state into the numbers and colours a box draws. Generic over the colour
//! type through [`Ctx`], so a table's resolution can be checked against a
//! palette whose every role answers with its own name.
//!
//! Tables are `serde_json::Value` because the JSON files are the schema:
//! a state merges over its base key by key, exactly as style.js merges two
//! objects, and an absent key reads the way an absent JS property does.

use serde_json::{Map, Value};

use crate::tokens::{self, Easing, Motion};

/// Every role a table must carry and its states. An empty list is a role
/// with one box; otherwise the FIRST state is the base the others merge
/// over, which is why `switch.track` reads `off` before `on`. The band
/// carries one paint per answer `bar.style` and its wallpaper reading give,
/// the frame the same (its band is a stretch of the ring once
/// `frame.thickness` is set), and an open cell on a band fills.
pub const ROLES: [(&str, &[&str]); 35] = [
    ("bar", BAND_PAINTS),
    ("frame", BAND_PAINTS),
    ("card", &["rest", "opaque"]),
    ("notification", &["rest", "critical", "flat", "flatCritical"]),
    ("popover", &[]),
    ("menu", &[]),
    ("cell", &["rest", "ghost", "hover", "active", "selected", "destructive", "warning", "ghostOpen"]),
    ("cell.mark", &[]),
    ("button.default", &["rest", "hover", "press"]),
    ("button.outline", &["rest", "hover", "press"]),
    ("button.ghost", &["rest", "hover", "press"]),
    ("button.selected", &["rest", "hover", "press"]),
    ("button.destructive", &["rest", "hover", "press"]),
    ("input", &["rest", "focus", "error"]),
    ("input.selection", &[]),
    ("switch.track", &["off", "on"]),
    ("switch.knob", &[]),
    ("track.groove", &[]),
    ("track.fill", &[]),
    ("track.notch", &[]),
    ("trough", &[]),
    ("segmented.chip", &[]),
    ("cursor", &[]),
    ("hero", &[]),
    ("bar.cell", &[]),
    ("backdrop.grid", &[]),
    ("backdrop.shards", &[]),
    ("backdrop.mark", &[]),
    ("scrim", &[]),
    ("separator", &[]),
    ("keycap", &[]),
    ("switcher", &[]),
    ("switcher.cell", &["rest", "selected"]),
    ("lock.ink", &["light", "dark"]),
    ("window", &["rest", "inactive"]),
];

const BAND_PAINTS: &[&str] = &["rest", "light", "dark", "translucentLight", "translucentDark", "maximized"];

pub const MOTION_KEYS: [&str; 4] = ["emerge", "arrive", "restack", "switcher"];
pub const WASH_KEYS: [&str; 4] = ["hover", "press", "filledHover", "filledPress"];
pub const RADIUS_STEPS: [&str; 5] = ["sm", "md", "lg", "xl", "pill"];

/// The three colours that are not palette roles.
pub const LITERAL_COLORS: [(&str, &str); 3] = [("black", "#000000"), ("white", "#ffffff"), ("transparent", "transparent")];

pub fn literal(name: &str) -> Option<&'static str> {
    LITERAL_COLORS.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

/// JavaScript truthiness, which is what every `if (raw.x)` in style.js
/// tests.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

fn num(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

/// `value || 0` on a number field.
fn num_or_zero(value: Option<&Value>) -> f64 {
    num(value).filter(|f| *f != 0.0 && !f.is_nan()).unwrap_or(0.0)
}

/// What a resolution needs from the live theme: the palette, the mode, the
/// surface alpha, the radius ladder and Qt's colour arithmetic.
pub trait Ctx {
    type Color: Clone + std::fmt::Debug + PartialEq;
    fn mode(&self) -> &str;
    fn surface_opacity(&self) -> f64;
    fn radius(&self, step: &str) -> Option<f64>;
    /// A palette role by name.
    fn color(&self, name: &str) -> Self::Color;
    /// One of [`LITERAL_COLORS`]' values.
    fn literal(&self, value: &'static str) -> Self::Color;
    /// `Qt.alpha(c, a)`.
    fn alpha(&self, c: &Self::Color, a: f64) -> Self::Color;
    /// `Qt.tint(c, over)`.
    fn tint(&self, c: &Self::Color, over: &Self::Color) -> Self::Color;
}

pub fn role_names() -> impl Iterator<Item = &'static str> {
    ROLES.iter().map(|(r, _)| *r)
}

pub fn states_for(role: &str) -> &'static [&'static str] {
    ROLES.iter().find(|(r, _)| *r == role).map_or(&[], |(_, s)| *s)
}

fn roles(style: &Value) -> Option<&Map<String, Value>> {
    style.get("roles").and_then(Value::as_object)
}

/// Whether a table describes a state at all.
pub fn has_state(style: &Value, role: &str, state: &str) -> bool {
    roles(style).and_then(|r| r.get(role)).is_some_and(|d| truthy(d.get(state)))
}

/// The raw entry for a role and a state, the state merged key by key over
/// the role's base. An unknown or empty state reads as the base.
pub fn entry(style: &Value, role: &str, state: Option<&str>) -> Option<Map<String, Value>> {
    let declared = roles(style)?.get(role).filter(|d| truthy(Some(d)))?;
    let states = states_for(role);
    if states.is_empty() {
        return declared.as_object().cloned();
    }
    let base = declared.get(states[0]).filter(|b| truthy(Some(b)))?.as_object()?;
    let over = match state {
        Some(s) if !s.is_empty() && s != states[0] && truthy(declared.get(s)) => declared.get(s)?,
        _ => return Some(base.clone()),
    };
    let mut out = base.clone();
    if let Some(over) = over.as_object() {
        for (k, v) in over {
            out.insert(k.clone(), v.clone());
        }
    }
    Some(out)
}

/// A number, `"surface"`, or a `{ light, dark }` pair the mode picks
/// between. Absent reads as opaque.
pub fn alpha_for(value: Option<&Value>, ctx: &impl Ctx) -> f64 {
    match value {
        None | Some(Value::Null) => 1.0,
        Some(Value::String(s)) if s == "surface" => ctx.surface_opacity(),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Object(pair)) => {
            let side = if ctx.mode() == "light" { "light" } else { "dark" };
            num(pair.get(side)).unwrap_or(f64::NAN)
        }
        Some(_) => 1.0,
    }
}

/// One colour: a literal or a palette role under the alpha the table asked
/// for. `transparent` never takes an alpha, since that would make black.
pub fn paint<C: Ctx>(name: Option<&Value>, alpha: Option<&Value>, ctx: &C) -> C::Color {
    let name = match name {
        None | Some(Value::Null) => return ctx.literal("transparent"),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    if name == "transparent" {
        return ctx.literal("transparent");
    }
    let base = match literal(&name) {
        Some(lit) => ctx.literal(lit),
        None => ctx.color(&name),
    };
    let a = alpha_for(alpha, ctx);
    if a >= 1.0 { base } else { ctx.alpha(&base, a) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Radius {
    Px(f64),
    /// Half the drawn item's own extent while the base radius is positive,
    /// which only the item knows.
    Pill,
}

pub fn radius_for(value: Option<&Value>, ctx: &impl Ctx) -> Radius {
    match value {
        Some(Value::Number(n)) => Radius::Px(n.as_f64().unwrap_or(0.0)),
        Some(Value::String(s)) if s == "pill" => Radius::Pill,
        Some(Value::String(s)) => Radius::Px(ctx.radius(s).unwrap_or(0.0)),
        _ => Radius::Px(0.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Cast,
    Ring,
    Hairline,
}

/// CSS box-shadow's own reading: a blur makes a cast, a spread alone a
/// ring, neither a hairline.
pub fn layer_kind(layer: &Value) -> LayerKind {
    if num(layer.get("blur")).is_some_and(|b| b > 0.0) {
        LayerKind::Cast
    } else if num(layer.get("spread")).is_some_and(|s| s > 0.0) {
        LayerKind::Ring
    } else {
        LayerKind::Hairline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hairline<C> {
    pub edge: Edge,
    pub thickness: f64,
    pub inset: bool,
    pub color: C,
}

/// Which edge a hairline lies along: a positive offset names the near edge
/// (top, left), a negative one the far edge. No offset, no line.
pub fn hairline(layer: &Value) -> Option<Hairline<()>> {
    let y = num_or_zero(layer.get("y"));
    let x = num_or_zero(layer.get("x"));
    let inset = truthy(layer.get("inset"));
    if y != 0.0 {
        let edge = if y > 0.0 { Edge::Top } else { Edge::Bottom };
        return Some(Hairline { edge, thickness: y.abs(), inset, color: () });
    }
    if x != 0.0 {
        let edge = if x > 0.0 { Edge::Left } else { Edge::Right };
        return Some(Hairline { edge, thickness: x.abs(), inset, color: () });
    }
    None
}

/// A layer list split by kind, each in the table's own order.
#[derive(Default)]
pub struct Layers<'a> {
    pub hairlines: Vec<&'a Value>,
    pub rings: Vec<&'a Value>,
    pub casts: Vec<&'a Value>,
}

pub fn layers(list: Option<&Value>) -> Layers<'_> {
    let mut out = Layers::default();
    for layer in list.and_then(Value::as_array).into_iter().flatten() {
        match layer_kind(layer) {
            LayerKind::Cast => out.casts.push(layer),
            LayerKind::Ring => out.rings.push(layer),
            LayerKind::Hairline => out.hairlines.push(layer),
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line<C> {
    pub color: C,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face<C> {
    pub from: C,
    pub to: C,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glow<C> {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub color: C,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ring<C> {
    pub spread: f64,
    pub color: C,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cast<C> {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub color: C,
}

/// A whole box, drawable. Every field is always present so a renderer
/// never guards for an absent one; transparent ink means the material says
/// nothing about the ink.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle<C> {
    pub fill: C,
    pub radius: Radius,
    pub border: Option<Line<C>>,
    pub face: Option<Face<C>>,
    pub wash: Option<C>,
    pub edge: Option<Line<C>>,
    pub ink: C,
    pub ink_shadow: Vec<Glow<C>>,
    pub hairlines: Vec<Hairline<C>>,
    pub rings: Vec<Ring<C>>,
    pub inset_rings: Vec<Ring<C>>,
    pub casts: Vec<Cast<C>>,
}

impl<C: Clone> BoxStyle<C> {
    fn empty(transparent: C) -> Self {
        Self {
            fill: transparent.clone(),
            radius: Radius::Px(0.0),
            border: None,
            face: None,
            wash: None,
            edge: None,
            ink: transparent,
            ink_shadow: Vec::new(),
            hairlines: Vec::new(),
            rings: Vec::new(),
            inset_rings: Vec::new(),
            casts: Vec::new(),
        }
    }
}

fn pair(value: Option<&Value>) -> (Option<&Value>, Option<&Value>) {
    (value.and_then(|v| v.get(0)), value.and_then(|v| v.get(1)))
}

fn line<C: Ctx>(raw: Option<&Value>, ctx: &C) -> Option<Line<C::Color>> {
    let raw = raw.filter(|r| truthy(Some(r)))?;
    Some(Line {
        color: paint(raw.get("color"), raw.get("alpha"), ctx),
        width: num(raw.get("width")).unwrap_or(0.0),
    })
}

/// The whole box: colours against the live palette, alphas against the
/// mode, the radius against the ladder, and `tint` folded into the fill
/// (the fill blended toward a colour and left opaque).
pub fn resolve<C: Ctx>(style: &Value, role: &str, state: Option<&str>, ctx: &C) -> BoxStyle<C::Color> {
    let mut out = BoxStyle::empty(ctx.literal("transparent"));
    let Some(raw) = entry(style, role, state) else {
        return out;
    };

    out.fill = paint(raw.get("fill"), raw.get("fillAlpha"), ctx);
    if truthy(raw.get("tint")) {
        let (color, alpha) = pair(raw.get("tint"));
        out.fill = ctx.tint(&out.fill, &paint(color, alpha, ctx));
    }
    out.radius = radius_for(raw.get("radius"), ctx);
    out.border = line(raw.get("border"), ctx);
    if let Some(face) = raw.get("face").filter(|f| truthy(Some(f))) {
        let (fc, fa) = pair(face.get("from"));
        let (tc, ta) = pair(face.get("to"));
        out.face = Some(Face { from: paint(fc, fa, ctx), to: paint(tc, ta, ctx) });
    }
    if let Some(wash) = raw.get("wash").filter(|w| truthy(Some(w))) {
        out.wash = Some(paint(wash.get("color"), wash.get("alpha"), ctx));
    }
    out.edge = line(raw.get("edge"), ctx);
    if truthy(raw.get("ink")) {
        let (color, alpha) = pair(raw.get("ink"));
        out.ink = paint(color, alpha, ctx);
    }
    for glow in raw.get("inkShadow").and_then(Value::as_array).into_iter().flatten() {
        out.ink_shadow.push(Glow {
            x: num_or_zero(glow.get("x")),
            y: num_or_zero(glow.get("y")),
            blur: num_or_zero(glow.get("blur")),
            color: paint(glow.get("color"), glow.get("alpha"), ctx),
        });
    }

    let split = layers(raw.get("layers"));
    for layer in split.hairlines {
        if let Some(h) = hairline(layer) {
            out.hairlines.push(Hairline {
                edge: h.edge,
                thickness: h.thickness,
                inset: h.inset,
                color: paint(layer.get("color"), layer.get("alpha"), ctx),
            });
        }
    }
    for layer in split.rings {
        let ring = Ring {
            spread: num(layer.get("spread")).unwrap_or(0.0),
            color: paint(layer.get("color"), layer.get("alpha"), ctx),
        };
        if truthy(layer.get("inset")) {
            out.inset_rings.push(ring);
        } else {
            out.rings.push(ring);
        }
    }
    for layer in split.casts {
        out.casts.push(Cast {
            x: num_or_zero(layer.get("x")),
            y: num_or_zero(layer.get("y")),
            blur: num(layer.get("blur")).unwrap_or(0.0),
            spread: num_or_zero(layer.get("spread")),
            color: paint(layer.get("color"), layer.get("alpha"), ctx),
        });
    }
    out
}

/// One clock: a number or a motion family's name on either side, and the
/// optional stagger window.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    pub duration: f64,
    pub curve: Easing,
    pub stagger: f64,
}

/// A key no table carries rides the spatial family. `m` comes in unzeroed:
/// the reduced-motion switch and the rig's scale apply on top.
pub fn motion(style: &Value, key: &str, m: &Motion) -> Clock {
    let raw = style.get("motion").and_then(|mo| mo.get(key));
    let duration = match raw.and_then(|r| r.get("duration")) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(m.spatial),
        Some(Value::String(s)) => m.family(s).unwrap_or(m.spatial),
        _ => m.spatial,
    };
    let spatial = || Easing::Bezier(tokens::curve("spatial").expect("spatial is in MOTION_CURVES").to_vec());
    let curve = match raw.and_then(|r| r.get("curve")) {
        Some(Value::Array(points)) if points.first().is_some_and(Value::is_array) => {
            stops(points).map_or_else(spatial, Easing::Stops)
        }
        Some(Value::Array(points)) => Easing::Bezier(points.iter().map(|p| p.as_f64().unwrap_or(f64::NAN)).collect()),
        Some(Value::String(s)) => tokens::curve(s).map_or_else(spatial, |c| Easing::Bezier(c.to_vec())),
        _ => spatial(),
    };
    let stagger = num(raw.and_then(|r| r.get("stagger"))).unwrap_or(0.0);
    Clock { duration, curve, stagger }
}

/// A stop list as a table writes it, `[[t, v], ...]`: `None` unless every
/// stop is two numbers and t climbs from 0 to 1 without stepping back.
pub fn stops(raw: &[Value]) -> Option<Vec<(f64, f64)>> {
    let stops = raw
        .iter()
        .map(|stop| match stop.as_array()?.as_slice() {
            [t, v] => Some((t.as_f64()?, v.as_f64()?)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let climbs = stops.windows(2).all(|w| w[0].0 <= w[1].0);
    let spans = stops.first().is_some_and(|s| s.0 == 0.0) && stops.last().is_some_and(|s| s.0 == 1.0);
    (stops.len() >= 2 && climbs && spans).then_some(stops)
}

/// One of the table's pointer washes, transparent for an unknown key.
pub fn wash<C: Ctx>(style: &Value, key: &str, ctx: &C) -> C::Color {
    match style.get("wash").and_then(|w| w.get(key)).filter(|e| truthy(Some(e))) {
        Some(e) => paint(e.get("color"), e.get("alpha"), ctx),
        None => ctx.literal("transparent"),
    }
}

/// The keyboard cursor composed over a box: its border in place of the
/// box's, and its halo only when `halo` asks, since a list draws one
/// travelling halo for every row it owns.
pub fn with_cursor<C: Clone>(b: &BoxStyle<C>, cursor: &BoxStyle<C>, halo: bool) -> BoxStyle<C> {
    let mut out = b.clone();
    if cursor.border.is_some() {
        out.border = cursor.border.clone();
    }
    if halo {
        out.rings.extend(cursor.rings.iter().cloned());
    }
    out
}

/// A number a role carries beside its box (the backdrop grid's pitch),
/// 0 when the table gives none.
pub fn metric(style: &Value, role: &str, key: &str) -> f64 {
    entry(style, role, None).and_then(|e| num(e.get(key))).filter(|n| n.is_finite()).unwrap_or(0.0)
}

/// Every box a table declares under [`ROLES`], with its path, in role order.
fn walk_boxes<'a>(style: &'a Value, mut take: impl FnMut(String, &'a Value)) {
    let Some(roles) = roles(style) else {
        return;
    };
    for role in role_names() {
        let Some(declared) = roles.get(role).filter(|d| truthy(Some(d))) else {
            continue;
        };
        let states = states_for(role);
        if states.is_empty() {
            take(role.to_owned(), declared);
            continue;
        }
        for state in states {
            if let Some(b) = declared.get(state).filter(|b| truthy(Some(b))) {
                take(format!("{role}.{state}"), b);
            }
        }
    }
}

/// Every ink-shadow layer a table names, for the validation test.
pub fn ink_shadow_layers(style: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    walk_boxes(style, |path, b| {
        for (g, layer) in b.get("inkShadow").and_then(Value::as_array).into_iter().flatten().enumerate() {
            out.push((format!("{path}.inkShadow[{g}]"), layer));
        }
    });
    out
}

/// Every colour name a table references, first use first.
pub fn color_names(style: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut take = |name: Option<&Value>| {
        if let Some(Value::String(s)) = name
            && !out.contains(s)
        {
            out.push(s.clone());
        }
    };
    walk_boxes(style, |_, b| {
        take(b.get("fill"));
        if truthy(b.get("border")) {
            take(b["border"].get("color"));
        }
        if truthy(b.get("face")) {
            take(b["face"].get("from").and_then(|f| f.get(0)));
            take(b["face"].get("to").and_then(|t| t.get(0)));
        }
        if truthy(b.get("wash")) {
            take(b["wash"].get("color"));
        }
        if truthy(b.get("edge")) {
            take(b["edge"].get("color"));
        }
        if truthy(b.get("tint")) {
            take(b["tint"].get(0));
        }
        if truthy(b.get("ink")) {
            take(b["ink"].get(0));
        }
        for glow in b.get("inkShadow").and_then(Value::as_array).into_iter().flatten() {
            take(glow.get("color"));
        }
        if truthy(b.get("shadow")) {
            take(b["shadow"].get("color"));
        }
        for layer in b.get("layers").and_then(Value::as_array).into_iter().flatten() {
            take(layer.get("color"));
        }
    });
    for key in WASH_KEYS {
        if let Some(w) = style.get("wash").and_then(|w| w.get(key)).filter(|w| truthy(Some(w))) {
            take(w.get("color"));
        }
    }
    out
}

/// Every alpha a table names, with its path.
pub fn alpha_values(style: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    fn take<'a>(out: &mut Vec<(String, &'a Value)>, path: String, value: Option<&'a Value>) {
        match value {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s == "surface" => {}
            Some(v) => out.push((path, v)),
        }
    }
    walk_boxes(style, |path, b| {
        take(&mut out, format!("{path}.fillAlpha"), b.get("fillAlpha"));
        if truthy(b.get("border")) {
            take(&mut out, format!("{path}.border.alpha"), b["border"].get("alpha"));
        }
        if truthy(b.get("face")) {
            take(&mut out, format!("{path}.face.from"), b["face"].get("from").and_then(|f| f.get(1)));
            take(&mut out, format!("{path}.face.to"), b["face"].get("to").and_then(|t| t.get(1)));
        }
        if truthy(b.get("wash")) {
            take(&mut out, format!("{path}.wash.alpha"), b["wash"].get("alpha"));
        }
        if truthy(b.get("edge")) {
            take(&mut out, format!("{path}.edge.alpha"), b["edge"].get("alpha"));
        }
        if truthy(b.get("tint")) {
            take(&mut out, format!("{path}.tint"), b["tint"].get(1));
        }
        if truthy(b.get("shadow")) {
            take(&mut out, format!("{path}.shadow.alpha"), b["shadow"].get("alpha"));
        }
        for (i, layer) in b.get("layers").and_then(Value::as_array).into_iter().flatten().enumerate() {
            take(&mut out, format!("{path}.layers[{i}]"), layer.get("alpha"));
        }
        for (g, glow) in b.get("inkShadow").and_then(Value::as_array).into_iter().flatten().enumerate() {
            take(&mut out, format!("{path}.inkShadow[{g}]"), glow.get("alpha"));
        }
    });
    for key in WASH_KEYS {
        if let Some(w) = style.get("wash").and_then(|w| w.get(key)).filter(|w| truthy(Some(w))) {
            take(&mut out, format!("wash.{key}"), w.get("alpha"));
        }
    }
    out
}

/// What a renderer draws a resolved box as, for a box of
/// `width` by `height`: the geometry every renderer of a box shares.
pub mod geometry {
    use super::{Cast, Edge};

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Rect {
        pub x: f64,
        pub y: f64,
        pub width: f64,
        pub height: f64,
        pub radius: f64,
    }

    /// The room the cast layer reaches past the box on every side.
    pub fn cast_pad<C>(casts: &[Cast<C>]) -> f64 {
        casts
            .iter()
            .map(|c| (c.blur + c.spread.max(0.0) + c.x.abs().max(c.y.abs())).ceil())
            .fold(0.0, f64::max)
    }

    /// The corner inside a border, held to what the box can fit.
    pub fn inner_radius(radius: f64, border_width: f64, width: f64, height: f64) -> f64 {
        (radius - border_width).min((width - border_width * 2.0) / 2.0).min((height - border_width * 2.0) / 2.0).max(0.0)
    }

    /// A ring is a filled rounded rectangle outset by its spread and drawn
    /// under the fill, which is what CSS paints for a spread with no blur.
    pub fn ring(width: f64, height: f64, radius: f64, spread: f64) -> Rect {
        Rect { x: -spread, y: -spread, width: width + spread * 2.0, height: height + spread * 2.0, radius: radius + spread }
    }

    /// One hairline: a clipped band along its edge, as deep as the inner
    /// corner, holding the inner outline stroked at the line's thickness so
    /// the line follows both arcs. Both rects are relative to the layer
    /// inside the border; the second is the stroke, relative to the band.
    pub fn hairline(edge: Edge, thickness: f64, layer_w: f64, layer_h: f64, inner_radius: f64) -> (Rect, Rect) {
        let sideways = matches!(edge, Edge::Left | Edge::Right);
        let depth = thickness.max(inner_radius);
        let width = if sideways { depth } else { layer_w };
        let height = if sideways { layer_h } else { depth };
        let x = if edge == Edge::Right { layer_w - width } else { 0.0 };
        let y = if edge == Edge::Bottom { layer_h - height } else { 0.0 };
        let band = Rect { x, y, width, height, radius: 0.0 };
        let stroke = Rect { x: -x, y: -y, width: layer_w, height: layer_h, radius: inner_radius };
        (band, stroke)
    }

    /// One cast's rect relative to the box and the spread it is drawn with:
    /// a negative spread shrinks the rect and its radius instead.
    pub fn cast<C>(width: f64, height: f64, radius: f64, cast: &Cast<C>) -> (Rect, f64) {
        let shrink = (-cast.spread).max(0.0);
        let rect = Rect {
            x: shrink,
            y: shrink,
            width: width - shrink * 2.0,
            height: height - shrink * 2.0,
            radius: (radius - shrink).max(0.0),
        };
        (rect, cast.spread.max(0.0))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::palette;
    use serde_json::json;

    /// A palette whose every role answers with its own name, so a resolved
    /// colour says which role it came from.
    pub(crate) struct Names {
        pub mode: &'static str,
    }

    pub(crate) fn fmt(a: f64) -> String {
        format!("{a}")
    }

    impl Ctx for Names {
        type Color = String;
        fn mode(&self) -> &str {
            self.mode
        }
        fn surface_opacity(&self) -> f64 {
            0.85
        }
        fn radius(&self, step: &str) -> Option<f64> {
            match step {
                "sm" => Some(6.0),
                "md" => Some(8.0),
                "lg" => Some(10.0),
                "xl" => Some(14.0),
                _ => None,
            }
        }
        fn color(&self, name: &str) -> String {
            format!("role:{name}")
        }
        fn literal(&self, value: &'static str) -> String {
            value.to_owned()
        }
        fn alpha(&self, c: &String, a: f64) -> String {
            format!("{c}@{}", fmt(a))
        }
        fn tint(&self, c: &String, over: &String) -> String {
            format!("{c}+{over}")
        }
    }

    fn ctx(mode: &'static str) -> Names {
        Names { mode }
    }

    fn tables() -> [(&'static str, &'static Value); 1] {
        [("formalshell", crate::table())]
    }

    const BLACK: &str = "#000000";
    const WHITE: &str = "#ffffff";
    const CLEAR: &str = "transparent";

    fn r(style: &Value, role: &str, state: Option<&str>, mode: &'static str) -> BoxStyle<String> {
        resolve(style, role, state, &ctx(mode))
    }

    // --- The tables

    #[test]
    fn every_table_carries_every_role_and_state() {
        assert!(role_names().count() > 0);
        for (name, style) in tables() {
            for role in role_names() {
                let declared = &style["roles"][role];
                assert!(truthy(Some(declared)), "{name} is missing the role {role}");
                for state in states_for(role) {
                    assert!(truthy(declared.get(*state)), "{name} is missing {role}.{state}");
                }
            }
        }
    }

    #[test]
    fn every_colour_a_table_names_is_a_palette_role_or_a_literal() {
        for (name, style) in tables() {
            let used = color_names(style);
            assert!(!used.is_empty());
            for c in used {
                let known = palette::COLOR_KEYS.contains(&c.as_str()) || literal(&c).is_some();
                assert!(known, "{name} names the unknown colour {c}");
            }
        }
    }

    #[test]
    fn every_alpha_is_a_fraction_in_both_modes() {
        for (name, style) in tables() {
            let alphas = alpha_values(style);
            assert!(!alphas.is_empty());
            for (path, value) in alphas {
                for mode in ["dark", "light"] {
                    let a = alpha_for(Some(value), &ctx(mode));
                    assert!(a.is_finite(), "{name} {path} is not a number in {mode}");
                    assert!((0.0..=1.0).contains(&a), "{name} {path} is {a} in {mode}");
                }
            }
        }
    }

    #[test]
    fn every_ink_shadow_layer_is_an_offset_a_blur_and_a_colour() {
        let mut seen = 0;
        for (name, style) in tables() {
            for (path, layer) in ink_shadow_layers(style) {
                let at = format!("{name} {path}");
                assert!(layer.is_object(), "{at} is not a layer");
                for field in ["x", "y", "blur"] {
                    let v = layer.get(field);
                    assert!(v.is_none() || v.and_then(Value::as_f64).is_some_and(f64::is_finite), "{at}.{field} is not a number");
                }
                assert!(layer.get("color").is_some_and(Value::is_string), "{at} names no colour");
                seen += 1;
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn every_radius_is_a_step_or_a_number() {
        for (name, style) in tables() {
            for role in role_names() {
                let states = states_for(role);
                let walk: Vec<Option<&str>> =
                    if states.is_empty() { vec![None] } else { states.iter().map(|s| Some(*s)).collect() };
                for state in walk {
                    let e = entry(style, role, state).expect("every role resolves");
                    match e.get("radius") {
                        None => {}
                        Some(Value::Number(_)) => {}
                        Some(Value::String(s)) => {
                            assert!(RADIUS_STEPS.contains(&s.as_str()), "{name} {role} has the radius {s}")
                        }
                        Some(v) => panic!("{name} {role} has the radius {v}"),
                    }
                }
            }
        }
    }

    #[test]
    fn every_table_declares_every_clock() {
        let families = tokens::motion_tokens(true);
        for (name, style) in tables() {
            for key in MOTION_KEYS {
                let e = style.get("motion").and_then(|m| m.get(key));
                assert!(truthy(e), "{name} is missing the clock {key}");
                let e = e.unwrap();
                match &e["duration"] {
                    Value::Number(n) => assert!(n.as_f64().unwrap() > 0.0, "{name} {key} has the duration {n}"),
                    Value::String(s) => assert!(families.family(s).is_some(), "{name} {key} has the duration {s}"),
                    v => panic!("{name} {key} has the duration {v}"),
                }
                match &e["curve"] {
                    Value::String(s) => assert!(tokens::curve(s).is_some(), "{name} {key} names the unknown curve {s}"),
                    Value::Array(c) if c.first().is_some_and(Value::is_array) => {
                        assert!(stops(c).is_some(), "{name} {key} has a stop list t does not climb 0 to 1 on");
                    }
                    Value::Array(c) => {
                        assert!(c.len() == 6 || c.len() == 12, "{name} {key} has {} control numbers", c.len());
                        assert_eq!(c[c.len() - 1].as_f64(), Some(1.0));
                        assert_eq!(c[c.len() - 2].as_f64(), Some(1.0));
                    }
                    v => panic!("{name} {key} has the curve {v}"),
                }
                let stagger = e.get("stagger");
                if let Some(s) = stagger {
                    assert!(s.as_f64().is_some_and(|s| s > 0.0), "{name} {key} has the stagger {s}");
                }
                assert_eq!(motion(style, key, &families).stagger, stagger.and_then(Value::as_f64).unwrap_or(0.0));
            }
        }
    }

    #[test]
    fn every_table_declares_the_washes() {
        for (name, style) in tables() {
            for key in WASH_KEYS {
                let w = style["wash"].get(key);
                assert!(truthy(w), "{name} is missing the wash {key}");
                assert!(w.unwrap()["color"].is_string());
            }
        }
    }

    #[test]
    fn the_marks_a_control_paints_come_off_the_table() {
        let style = crate::table();
        let mark = r(style, "cell.mark", None, "dark");
        assert_eq!(mark.fill, "role:primary");
        assert_eq!(mark.radius, Radius::Px(6.0));
        assert_eq!(r(style, "track.notch", None, "dark").fill, "role:background");
        assert_eq!(r(style, "input.selection", None, "dark").fill, "role:primary");
    }

    #[test]
    fn the_cursor_composes_over_a_box() {
        let style = crate::table();
        let ghost = r(style, "button.ghost", Some("rest"), "dark");
        let cursor = r(style, "cursor", None, "dark");

        let haloed = with_cursor(&ghost, &cursor, true);
        assert_eq!(haloed.fill, ghost.fill);
        assert_eq!(haloed.radius, ghost.radius);
        assert_eq!(haloed.border.as_ref().unwrap().color, "role:ring");
        assert_eq!(haloed.rings.len(), 1);
        assert_eq!(haloed.rings[0].spread, 2.0);

        let bordered = with_cursor(&ghost, &cursor, false);
        assert_eq!(bordered.border.as_ref().unwrap().color, "role:ring");
        assert_eq!(bordered.rings.len(), 0);

        assert_eq!(ghost.border, None);
        assert_eq!(ghost.rings.len(), 0);
    }

    // --- The table's own values

    #[test]
    fn the_pantheon_button_is_raised() {
        let p = crate::table();
        let button = r(p, "button.outline", Some("rest"), "light");
        assert_eq!(button.fill, "role:secondary");
        assert_eq!(button.radius, Radius::Px(3.0));
        let face = button.face.clone().unwrap();
        assert_eq!(face.from, "#ffffff@0.2");
        assert_eq!(face.to, "#ffffff@0");
        assert_eq!(button.border.as_ref().unwrap().color, "#000000@0.2");
        assert_eq!(button.hairlines.len(), 4);
        assert_eq!(button.hairlines[0].edge, Edge::Top);
        assert_eq!(button.hairlines[0].color, "#ffffff@0.3");
        assert_eq!(button.casts.len(), 2);
        assert_eq!(button.casts[1].blur, 2.0);
        assert_eq!(button.casts[1].color, "#000000@0.08");

        let press = r(p, "button.outline", Some("press"), "light");
        assert_eq!(press.face, None);
        assert_eq!(press.casts.len(), 0);
        assert_eq!(press.hairlines.len(), 1);
        assert_eq!(press.hairlines[0].color, "#000000@0.1");
    }

    #[test]
    fn the_pantheon_card_carries_every_layer_kind() {
        let p = crate::table();
        let card = r(p, "card", None, "dark");
        assert_eq!(card.fill, "role:card@0.85");
        assert_eq!(card.radius, Radius::Px(9.0));
        let border = card.border.clone().unwrap();
        assert_eq!(border.color, "#000000@0.75");
        assert_eq!(border.width, 1.0);
        assert_eq!(card.hairlines.len(), 4);
        assert_eq!(card.rings.len(), 0);
        assert_eq!(card.casts.len(), 2);
        assert_eq!(card.casts[0].y, 3.0);
        assert_eq!(card.casts[0].blur, 4.0);
        assert_eq!(card.casts[0].color, "#000000@0.25");
        assert_eq!(card.casts[1].spread, -3.0);
        assert_eq!(card.casts[1].color, "#000000@0.45");
        assert_eq!(r(p, "card", Some("opaque"), "dark").fill, "role:card");
    }

    #[test]
    fn the_pantheon_highlight_is_the_product_in_dark() {
        let p = crate::table();
        let light = r(p, "card", None, "light");
        let dark = r(p, "card", None, "dark");
        assert_eq!(light.hairlines[0].color, "#ffffff@0.3");
        assert_eq!(dark.hairlines[0].color, "#ffffff@0.06");
        assert_eq!(light.hairlines[1].color, "#ffffff@0.2");
        assert_eq!(dark.hairlines[1].color, "#ffffff@0.04");
        assert_eq!(dark.hairlines[2].color, "#ffffff@0.014");
        assert_eq!(r(p, "button.outline", Some("rest"), "dark").face.unwrap().from, "#ffffff@0.04");
    }

    #[test]
    fn the_pantheon_cursor_is_a_two_pixel_halo() {
        let p = crate::table();
        let cursor = r(p, "cursor", None, "dark");
        assert_eq!(cursor.border.unwrap().color, "role:ring");
        assert_eq!(cursor.rings.len(), 1);
        assert_eq!(cursor.rings[0].spread, 2.0);
        assert_eq!(cursor.rings[0].color, "role:ring@0.3");

        let focus = r(p, "input", Some("focus"), "dark");
        assert_eq!(focus.rings.len(), 1);
        assert_eq!(focus.rings[0].spread, 2.0);
        assert_eq!(focus.hairlines.len(), 1);
    }

    #[test]
    fn the_pantheon_bubble_flattens_inside_the_centre() {
        let p = crate::table();
        let bubble = r(p, "notification", Some("rest"), "dark");
        assert_eq!(bubble.fill, "role:card");
        assert_eq!(bubble.radius, Radius::Px(9.0));
        assert_eq!(bubble.casts.len(), 2);
        assert_eq!(bubble.casts[1].blur, 9.0);
        assert_eq!(bubble.border.unwrap().color, "#000000@0.75");
        assert_eq!(r(p, "notification", Some("critical"), "dark").border.unwrap().color, "role:destructive");

        let flat = r(p, "notification", Some("flat"), "dark");
        assert_eq!(flat.fill, CLEAR);
        assert_eq!(flat.border, None);
        assert_eq!(flat.casts.len(), 0);
        assert_eq!(flat.hairlines.len(), 0);
        assert_eq!(r(p, "notification", Some("flatCritical"), "dark").border.unwrap().color, "role:destructive");
    }

    #[test]
    fn the_pantheon_scrim_is_gala_s_dim() {
        let scrim = r(crate::table(), "scrim", None, "dark");
        assert_eq!(scrim.fill, format!("{BLACK}@{}", fmt(125.0 / 255.0)));
    }

    #[test]
    fn the_pantheon_band_has_a_paint_per_reading() {
        let p = crate::table();
        let light = r(p, "bar", Some("light"), "dark");
        assert_eq!(light.fill, CLEAR);
        assert_eq!(light.ink, WHITE);
        assert_eq!(light.casts.len(), 0);
        assert_eq!(light.ink_shadow.len(), 2);
        assert_eq!(light.ink_shadow[0].color, format!("{BLACK}@0.3"));
        assert_eq!(light.ink_shadow[0].blur, 2.0);
        assert_eq!(light.ink_shadow[0].y, 0.0);
        assert_eq!(light.ink_shadow[1].color, format!("{BLACK}@0.6"));
        assert_eq!(light.ink_shadow[1].blur, 2.0);
        assert_eq!(light.ink_shadow[1].y, 1.0);

        let dark = r(p, "bar", Some("dark"), "dark");
        assert_eq!(dark.fill, CLEAR);
        assert_eq!(dark.ink, format!("{BLACK}@0.65"));
        assert_eq!(dark.ink_shadow.len(), 0);

        let td = r(p, "bar", Some("translucentDark"), "dark");
        assert_eq!(td.fill, format!("{BLACK}@0.3"));
        assert_eq!(td.ink, WHITE);
        assert_eq!(td.ink_shadow.len(), 2);
        assert_eq!(td.ink_shadow[0].color, format!("{BLACK}@0.15"));
        assert_eq!(td.ink_shadow[1].color, format!("{BLACK}@0.3"));

        let tl = r(p, "bar", Some("translucentLight"), "dark");
        assert_eq!(tl.fill, format!("{WHITE}@0.5"));
        assert_eq!(tl.ink, format!("{BLACK}@0.65"));
        assert_eq!(tl.ink_shadow.len(), 0);
        assert_eq!(tl.hairlines.len(), 2);
        assert_eq!(tl.hairlines[0].color, format!("{WHITE}@0.15"));
        assert_eq!(tl.hairlines[1].edge, Edge::Bottom);
        assert_eq!(tl.hairlines[1].color, format!("{WHITE}@0.03"));

        let maximized = r(p, "bar", Some("maximized"), "dark");
        assert_eq!(maximized.fill, BLACK);
        assert_eq!(maximized.ink, WHITE);
        assert_eq!(maximized.ink_shadow.len(), 2);
        assert_eq!(maximized.ink_shadow[1].color, format!("{BLACK}@0.6"));

        assert_eq!(r(p, "bar", None, "dark").edge.unwrap().width, 0.0);
    }

    #[test]
    fn the_pantheon_ring_takes_the_bands_paint() {
        let p = crate::table();
        for bare in ["light", "dark"] {
            let b = r(p, "frame", Some(bare), "dark");
            assert_eq!(b.fill, CLEAR, "{bare} draws a fill");
            let border = b.border.unwrap();
            assert_eq!(border.color, CLEAR, "{bare} draws a line");
            assert_eq!(border.width, 1.0);
        }
        let td = r(p, "frame", Some("translucentDark"), "dark");
        assert_eq!(td.fill, format!("{BLACK}@0.3"));
        assert_eq!(td.border.unwrap().color, format!("{BLACK}@0.75"));
        assert_eq!(td.casts.len(), 0);

        let tl = r(p, "frame", Some("translucentLight"), "dark");
        assert_eq!(tl.fill, format!("{WHITE}@0.5"));
        assert_eq!(tl.border.unwrap().color, format!("{WHITE}@0.15"));

        assert_eq!(r(p, "frame", Some("maximized"), "dark").fill, BLACK);
        assert_eq!(r(p, "frame", Some(""), "dark").fill, CLEAR);
    }

    #[test]
    fn every_table_draws_solid_as_an_opaque_band_on_a_line() {
        for (name, style) in tables() {
            for mode in ["dark", "light"] {
                let solid = r(style, "bar", Some("maximized"), mode);
                assert_eq!(solid.fill, BLACK, "{name} {mode}");
                let edge = solid.edge.unwrap_or_else(|| panic!("{name} draws solid with no line"));
                assert_eq!(edge.width, 1.0, "{name} {mode}");
                for state in ["light", "dark"] {
                    assert_eq!(r(style, "bar", Some(state), mode).fill, CLEAR, "{name} {state} {mode}");
                }
            }
        }
    }

    #[test]
    fn the_pantheon_open_indicator_fills_its_cell() {
        let p = crate::table();
        let open = r(p, "cell", Some("ghostOpen"), "light");
        assert_eq!(open.fill, format!("{WHITE}@0.6"));
        assert_eq!(open.border, None);
        assert_eq!(open.radius, Radius::Px(3.0));
        assert_eq!(r(p, "cell", Some("ghostOpen"), "dark").fill, format!("{WHITE}@0.12"));
    }

    #[test]
    fn the_pantheon_switcher_is_galas_card() {
        let p = crate::table();
        let card = r(p, "switcher", None, "dark");
        assert_eq!(card.fill, "role:background@0.6");
        assert_eq!(card.radius, Radius::Px(9.0));
        assert_eq!(card.border.unwrap().color, "#000000@0.75");
        assert_eq!(card.rings.len(), 0);
        assert_eq!(card.inset_rings.len(), 1);
        assert_eq!(card.inset_rings[0].spread, 1.5);
        assert_eq!(card.inset_rings[0].color, "#ffffff@0.3");
        assert_eq!(r(p, "switcher", None, "light").inset_rings[0].color, "#ffffff@0.3");

        let selected = r(p, "cell", Some("selected"), "dark");
        assert_eq!(selected.fill, "role:accent");
        assert_eq!(selected.radius, Radius::Px(3.0));
    }

    // --- The resolver

    #[test]
    fn a_state_absent_from_a_role_reads_as_its_base() {
        let m = crate::table();
        let rest = r(m, "cell", Some("rest"), "dark");
        let unknown = r(m, "cell", Some("nonsense"), "dark");
        assert_eq!(unknown.fill, rest.fill);
        assert_eq!(unknown.radius, rest.radius);
        assert_eq!(r(m, "cell", None, "dark").fill, rest.fill);
    }

    #[test]
    fn a_partial_state_merges_over_its_base() {
        let m = crate::table();
        let hover = r(m, "cell", Some("hover"), "dark");
        assert_eq!(hover.fill, "role:card@0.85");
        assert_eq!(hover.radius, Radius::Px(3.0));
        assert_eq!(hover.wash.as_deref(), Some("role:foreground@0.15"));

        let ghost = r(m, "cell", Some("ghost"), "dark");
        assert_eq!(ghost.fill, CLEAR);
        assert_eq!(ghost.border, None);
        assert_eq!(ghost.radius, Radius::Px(3.0));

        let on = r(m, "switch.track", Some("on"), "dark");
        assert_eq!(on.fill, "role:primary@0.4");
        assert_eq!(on.radius, Radius::Px(16.0));
    }

    #[test]
    fn an_alpha_pair_resolves_per_mode() {
        let pair = json!({ "light": 0.2, "dark": 0.3 });
        assert_eq!(alpha_for(Some(&pair), &ctx("dark")), 0.3);
        assert_eq!(alpha_for(Some(&pair), &ctx("light")), 0.2);
    }

    #[test]
    fn the_filled_steps_match_across_modes() {
        let wash = &crate::table()["wash"];
        assert_eq!(
            alpha_for(wash["filledHover"].get("alpha"), &ctx("dark")),
            alpha_for(wash["filledHover"].get("alpha"), &ctx("light"))
        );
        assert!(
            alpha_for(wash["filledPress"].get("alpha"), &ctx("dark"))
                > alpha_for(wash["filledHover"].get("alpha"), &ctx("dark"))
        );
    }

    #[test]
    fn an_unknown_mode_reads_as_dark() {
        let pair = json!({ "light": 0.2, "dark": 0.3 });
        assert_eq!(alpha_for(Some(&pair), &ctx("")), 0.3);
    }

    #[test]
    fn surface_takes_the_theme_alpha_and_an_absent_alpha_is_opaque() {
        assert_eq!(alpha_for(Some(&json!("surface")), &ctx("dark")), 0.85);
        assert_eq!(alpha_for(None, &ctx("dark")), 1.0);
        assert_eq!(alpha_for(Some(&json!(0.2)), &ctx("light")), 0.2);
    }

    #[test]
    fn a_tint_folds_into_the_fill_and_leaves_it_opaque() {
        let m = crate::table();
        let press = r(m, "button.default", Some("press"), "dark");
        assert_eq!(press.fill, "role:primary+role:background@0.18");
        assert_eq!(press.wash, None);
        assert_eq!(r(m, "button.ghost", Some("press"), "dark").wash.as_deref(), Some("role:foreground@0.22"));
    }

    #[test]
    fn a_radius_step_resolves_and_pill_survives() {
        let c = ctx("dark");
        assert_eq!(radius_for(Some(&json!("sm")), &c), Radius::Px(6.0));
        assert_eq!(radius_for(Some(&json!("xl")), &c), Radius::Px(14.0));
        assert_eq!(radius_for(Some(&json!(3)), &c), Radius::Px(3.0));
        assert_eq!(radius_for(Some(&json!("pill")), &c), Radius::Pill);
        assert_eq!(radius_for(None, &c), Radius::Px(0.0));
    }

    #[test]
    fn a_layer_is_read_by_its_blur_and_its_spread() {
        assert_eq!(layer_kind(&json!({ "blur": 4, "color": "black" })), LayerKind::Cast);
        assert_eq!(layer_kind(&json!({ "blur": 4, "spread": 2, "color": "black" })), LayerKind::Cast);
        assert_eq!(layer_kind(&json!({ "spread": 1, "color": "black" })), LayerKind::Ring);
        assert_eq!(layer_kind(&json!({ "inset": true, "y": 1, "color": "white" })), LayerKind::Hairline);

        let list = json!([
            { "inset": true, "y": 1, "color": "white", "alpha": 0.3 },
            { "spread": 1, "color": "black", "alpha": 0.1 },
            { "y": 3, "blur": 4, "color": "black", "alpha": 0.15 }
        ]);
        let split = layers(Some(&list));
        assert_eq!(split.hairlines.len(), 1);
        assert_eq!(split.rings.len(), 1);
        assert_eq!(split.casts.len(), 1);
        assert_eq!(layers(None).casts.len(), 0);
    }

    #[test]
    fn a_hairline_names_one_edge() {
        assert_eq!(hairline(&json!({ "inset": true, "y": 1 })).unwrap().edge, Edge::Top);
        assert_eq!(hairline(&json!({ "inset": true, "y": -2 })).unwrap().edge, Edge::Bottom);
        assert_eq!(hairline(&json!({ "inset": true, "y": -2 })).unwrap().thickness, 2.0);
        assert_eq!(hairline(&json!({ "inset": true, "x": 1 })).unwrap().edge, Edge::Left);
        assert_eq!(hairline(&json!({ "inset": true, "x": -1 })).unwrap().edge, Edge::Right);
        assert!(hairline(&json!({ "inset": true, "x": -1 })).unwrap().inset);
        assert!(!hairline(&json!({ "x": 1 })).unwrap().inset);
        assert_eq!(hairline(&json!({ "inset": true })), None);
    }

    #[test]
    fn an_unknown_role_resolves_to_an_empty_box() {
        let b = r(crate::table(), "nonsense", None, "dark");
        assert_eq!(b.fill, CLEAR);
        assert_eq!(b.radius, Radius::Px(0.0));
        assert_eq!(b.border, None);
        assert_eq!(b.hairlines.len(), 0);
    }

    #[test]
    fn a_wash_resolves_off_the_table() {
        let m = crate::table();
        assert_eq!(wash(m, "hover", &ctx("dark")), "role:foreground@0.15");
        assert_eq!(wash(m, "hover", &ctx("light")), "role:foreground@0.15");
        assert_eq!(wash(m, "filledPress", &ctx("dark")), "role:background@0.18");
        assert_eq!(wash(m, "nonsense", &ctx("dark")), CLEAR);
    }

    // --- The window

    #[test]
    fn every_table_declares_a_window_hyprland_can_draw() {
        for (name, style) in tables() {
            let focused = entry(style, "window", Some("rest")).unwrap();
            let backdrop = entry(style, "window", Some("inactive")).unwrap();

            let border = focused.get("border").expect("a window frame");
            let width = border["width"].as_f64().unwrap();
            assert!((0.0..=20.0).contains(&width), "{name} frames a window at {width}px");
            assert!(border["color"].is_string());

            let cast = focused.get("shadow").expect("a window shadow");
            assert!(cast["enabled"].is_boolean());
            let range = cast["range"].as_f64().unwrap();
            assert!((0.0..=100.0).contains(&range), "{name} casts at range {range}");
            let power = cast["renderPower"].as_f64().unwrap();
            assert!((1.0..=4.0).contains(&power), "{name} casts at power {power}");
            let offset = cast["offset"].as_array().unwrap();
            assert_eq!(offset.len(), 2);
            for o in offset {
                assert!((-250.0..=250.0).contains(&o.as_f64().unwrap()), "{name} casts at offset {o}");
            }
            assert!(
                backdrop.get("shadow").is_some_and(|s| s["color"].is_string()),
                "{name} declares no backdrop shadow colour"
            );
        }
    }

    #[test]
    fn the_pantheon_window_is_elementarys_shadow() {
        let focused = entry(crate::table(), "window", Some("rest")).unwrap();
        assert_eq!(focused["border"]["color"], "border");
        assert_eq!(focused["border"]["width"], 1);
        assert_eq!(focused["shadow"]["enabled"], true);
        assert_eq!(focused["shadow"]["range"], 24);
        assert_eq!(focused["shadow"]["renderPower"], 3);
        assert_eq!(focused["shadow"]["offset"][1], 6);
        assert_eq!(focused["shadow"]["alpha"], 0.35);
        assert_eq!(entry(crate::table(), "window", Some("inactive")).unwrap()["shadow"]["alpha"], 0.25);
    }

    #[test]
    fn a_stop_list_curve_is_read_as_stops() {
        let families = tokens::motion_tokens(true);
        let table = json!({ "motion": { "emerge": { "duration": 1200, "curve": [[0, 0], [0.4, 1.42], [0.7, 0.96], [1, 1]] } } });
        let clock = motion(&table, "emerge", &families);
        assert_eq!(clock.duration, 1200.0);
        assert_eq!(clock.curve, Easing::Stops(vec![(0.0, 0.0), (0.4, 1.42), (0.7, 0.96), (1.0, 1.0)]));
    }

    #[test]
    fn a_stop_list_t_does_not_climb_0_to_1_on_falls_back_to_spatial() {
        let families = tokens::motion_tokens(true);
        let spatial = Easing::Bezier(tokens::curve("spatial").unwrap().to_vec());
        for curve in [
            json!([[0, 0], [0.6, 1.2], [0.5, 1], [1, 1]]),
            json!([[0.1, 0], [1, 1]]),
            json!([[0, 0], [0.9, 1]]),
            json!([[0, 0]]),
            json!([[0, 0], [0.5], [1, 1]]),
            json!([[0, 0], ["0.5", 1], [1, 1]]),
        ] {
            let table = json!({ "motion": { "emerge": { "curve": curve } } });
            assert_eq!(motion(&table, "emerge", &families).curve, spatial, "{curve}");
        }
        assert!(stops(&[json!([0, 0]), json!([0.5, 0.5]), json!([0.5, 0.8]), json!([1, 1])]).is_some());
    }
}
