// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! The radio atlas globe: near-side perspective projection, kinetic rotation,
//! country geometry lookups and the station list helpers.
//!
//! Angles are degrees at the API and radians inside. Where the JS ran
//! arguments through `Number()`, a NaN argument takes the same path here.

use std::collections::HashMap;

use serde_json::Value;

use super::stations::Station;

const RADIANS: f64 = std::f64::consts::PI / 180.0;
const DEGREES: f64 = 180.0 / std::f64::consts::PI;

pub(crate) fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    maximum.min(value).max(minimum)
}

pub fn wrap_longitude(value: f64) -> f64 {
    let mut wrapped = (value + 180.0) % 360.0;
    if wrapped < 0.0 {
        wrapped += 360.0;
    }
    wrapped - 180.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LimitedVelocity {
    pub x: f64,
    pub y: f64,
    pub speed: f64,
}

pub(crate) fn limit_kinetic_velocity(x: f64, y: f64, maximum_speed: f64) -> LimitedVelocity {
    let zero = LimitedVelocity {
        x: 0.0,
        y: 0.0,
        speed: 0.0,
    };
    if !x.is_finite() || !y.is_finite() || !maximum_speed.is_finite() || maximum_speed <= 0.0 {
        return zero;
    }
    let speed = (x * x + y * y).sqrt();
    if !speed.is_finite() || speed <= 0.0 {
        return zero;
    }
    if speed <= maximum_speed {
        return LimitedVelocity { x, y, speed };
    }
    let ratio = maximum_speed / speed;
    LimitedVelocity {
        x: x * ratio,
        y: y * ratio,
        speed: maximum_speed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Launch {
    pub x: f64,
    pub y: f64,
    pub active: bool,
}

/// `minimum_speed` NaN reads as 0.
pub fn kinetic_launch_velocity(x: f64, y: f64, minimum_speed: f64, maximum_speed: f64) -> Launch {
    let limited = limit_kinetic_velocity(x, y, maximum_speed);
    let threshold = if minimum_speed.is_nan() {
        0.0
    } else {
        minimum_speed
    }
    .max(0.0);
    let active = limited.speed >= threshold;
    Launch {
        x: if active { limited.x } else { 0.0 },
        y: if active { limited.y } else { 0.0 },
        active,
    }
}

fn finite_or_zero(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KineticState {
    pub longitude: f64,
    pub latitude: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
}

/// `None` is a key the caller left out.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KineticOptions {
    pub deceleration: Option<f64>,
    pub scale: Option<f64>,
    pub longitude_sensitivity: Option<f64>,
    pub latitude_sensitivity: Option<f64>,
    pub minimum_latitude: Option<f64>,
    pub maximum_latitude: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticStep {
    pub longitude: f64,
    pub latitude: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub active: bool,
}

fn opt_or(v: Option<f64>, fallback: f64) -> f64 {
    match v {
        Some(v) if v.is_finite() => v,
        _ => fallback,
    }
}

pub fn advance_kinetic_rotation(
    state: KineticState,
    elapsed_seconds: f64,
    options: KineticOptions,
) -> KineticStep {
    let longitude = finite_or_zero(state.longitude);
    let latitude = finite_or_zero(state.latitude);
    let velocity_x = finite_or_zero(state.velocity_x);
    let velocity_y = finite_or_zero(state.velocity_y);
    let elapsed = elapsed_seconds;

    let speed = (velocity_x * velocity_x + velocity_y * velocity_y).sqrt();
    if !speed.is_finite() || speed <= 0.0 || !elapsed.is_finite() || elapsed <= 0.0 {
        return KineticStep {
            longitude: wrap_longitude(longitude),
            latitude,
            velocity_x,
            velocity_y,
            active: speed > 0.0,
        };
    }

    let mut deceleration = options.deceleration.unwrap_or(f64::NAN);
    if !deceleration.is_finite() || deceleration < 0.0 {
        deceleration = 0.0;
    }
    let active_time = if deceleration > 0.0 {
        elapsed.min(speed / deceleration)
    } else {
        elapsed
    };
    let next_speed = (speed - deceleration * active_time).max(0.0);
    let distance = (speed + next_speed) * active_time / 2.0;
    let direction_x = velocity_x / speed;
    let direction_y = velocity_y / speed;
    let delta_x = direction_x * distance;
    let delta_y = direction_y * distance;

    let mut scale = options.scale.unwrap_or(f64::NAN);
    if !scale.is_finite() || scale <= 0.0 {
        scale = 1.0;
    }
    let longitude_sensitivity = opt_or(options.longitude_sensitivity, 0.0);
    let latitude_sensitivity = opt_or(options.latitude_sensitivity, 0.0);
    let minimum_latitude = opt_or(options.minimum_latitude, -78.0);
    let maximum_latitude = opt_or(options.maximum_latitude, 78.0);

    let next_longitude = wrap_longitude(longitude - delta_x * longitude_sensitivity / scale);
    let next_latitude = clamp(
        latitude + delta_y * latitude_sensitivity / scale,
        minimum_latitude,
        maximum_latitude,
    );
    let next_velocity_x = direction_x * next_speed;
    let mut next_velocity_y = direction_y * next_speed;
    if (next_latitude >= maximum_latitude && next_velocity_y > 0.0)
        || (next_latitude <= minimum_latitude && next_velocity_y < 0.0)
    {
        next_velocity_y = 0.0;
    }

    let remaining_speed =
        (next_velocity_x * next_velocity_x + next_velocity_y * next_velocity_y).sqrt();
    KineticStep {
        longitude: next_longitude,
        latitude: next_latitude,
        velocity_x: next_velocity_x,
        velocity_y: next_velocity_y,
        active: remaining_speed > 1e-9,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub(crate) fn project(
    latitude: f64,
    longitude: f64,
    centre_latitude: f64,
    centre_longitude: f64,
) -> Point3 {
    let phi = latitude * RADIANS;
    let lambda = wrap_longitude(longitude - centre_longitude) * RADIANS;
    let phi0 = centre_latitude * RADIANS;
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());
    let (cos_phi0, sin_phi0) = (phi0.cos(), phi0.sin());
    Point3 {
        x: cos_phi * lambda.sin(),
        y: cos_phi0 * sin_phi - sin_phi0 * cos_phi * lambda.cos(),
        z: sin_phi0 * sin_phi + cos_phi0 * cos_phi * lambda.cos(),
    }
}

// The near-side perspective the globe is drawn in: a camera `distance` globe
// radii from the centre, looking at (centre_latitude, centre_longitude).
// `project` is the unit vector in the view frame, z toward the camera; a
// point is on the near side when z (the cosine of its angular distance from
// the view centre) is at least 1 / distance, and lands on screen at
// (x, y) * perspective_scale(z, distance), in units of the view radius.
pub(crate) const START_DISTANCE: f64 = 3.2;
pub(crate) const MINIMUM_DISTANCE: f64 = 1.05;
pub(crate) const VIEW_FILL: f64 = 0.44;

pub fn view_distance(scale: f64) -> f64 {
    let zoom = if !scale.is_finite() || scale <= 0.0 {
        1.0
    } else {
        scale
    };
    MINIMUM_DISTANCE.max(1.0 + (START_DISTANCE - 1.0) / zoom)
}

pub fn horizon_ratio(distance: f64) -> f64 {
    ((distance - 1.0) / (distance + 1.0)).sqrt()
}

/// Pixels per unit on the tangent plane. Sized so that at scale 1 the horizon
/// disc fills the pane's share the orthographic disc did, and growing with
/// scale as the camera closes in.
pub fn view_radius(width: f64, height: f64, scale: f64) -> f64 {
    width.min(height) * VIEW_FILL * scale / horizon_ratio(START_DISTANCE)
}

pub fn horizon_radius(width: f64, height: f64, scale: f64) -> f64 {
    view_radius(width, height, scale) * horizon_ratio(view_distance(scale))
}

pub(crate) fn visible_depth(depth: f64, distance: f64) -> bool {
    depth >= 1.0 / distance
}

pub(crate) fn perspective_scale(depth: f64, distance: f64) -> f64 {
    (distance - 1.0) / (distance - depth)
}

/// 0 at the horizon, 1 at the view centre: what dots are sized and faded by.
pub fn horizon_depth(depth: f64, distance: f64) -> f64 {
    let horizon = 1.0 / distance;
    clamp((depth - horizon) / (1.0 - horizon), 0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PerspectivePoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub visible: bool,
}

pub(crate) fn project_perspective(
    latitude: f64,
    longitude: f64,
    centre_latitude: f64,
    centre_longitude: f64,
    distance: f64,
) -> PerspectivePoint {
    let point = project(latitude, longitude, centre_latitude, centre_longitude);
    let k = perspective_scale(point.z, distance);
    PerspectivePoint {
        x: point.x * k,
        y: point.y * k,
        z: point.z,
        visible: visible_depth(point.z, distance),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub latitude: f64,
    pub longitude: f64,
}

/// The exact inverse of `project_perspective`: the ray from the camera through
/// the tangent-plane point (x, y) hits the sphere at its nearer root, or
/// misses it and there is nothing under that pixel.
pub fn unproject(
    x: f64,
    y: f64,
    centre_latitude: f64,
    centre_longitude: f64,
    distance: f64,
) -> Option<LatLon> {
    let d = distance;
    let a = x * x + y * y + (d - 1.0) * (d - 1.0);
    let discriminant = d * d * (d - 1.0) * (d - 1.0) - a * (d * d - 1.0);
    if !discriminant.is_finite() || discriminant < 0.0 {
        return None;
    }
    let t = (d * (d - 1.0) - discriminant.sqrt()) / a;
    let px = t * x;
    let py = t * y;
    let pz = d - t * (d - 1.0);
    if pz < 1.0 / d - 1e-9 {
        return None;
    }
    let phi0 = centre_latitude * RADIANS;
    let (cos_phi0, sin_phi0) = (phi0.cos(), phi0.sin());
    let latitude = clamp(py * cos_phi0 + pz * sin_phi0, -1.0, 1.0).asin();
    let longitude = centre_longitude * RADIANS + px.atan2(pz * cos_phi0 - py * sin_phi0);
    Some(LatLon {
        latitude: latitude * DEGREES,
        longitude: wrap_longitude(longitude * DEGREES),
    })
}

/// A ring is `[longitude, latitude]` pairs; a coordinate JS read as NaN stays
/// NaN.
pub(crate) type Ring = Vec<[f64; 2]>;
/// An outer ring followed by holes.
pub(crate) type Polygon = Vec<Ring>;

pub(crate) fn point_in_ring(longitude: f64, latitude: f64, ring: &[[f64; 2]]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let [xi, yi] = ring[i];
        let [xj, yj] = ring[j];
        let dy = yj - yi;
        let dy = if dy == 0.0 || dy.is_nan() { 1e-12 } else { dy };
        let crosses =
            (yi > latitude) != (yj > latitude) && longitude < (xj - xi) * (latitude - yi) / dy + xi;
        if crosses {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub(crate) fn point_in_polygon(longitude: f64, latitude: f64, polygon: &[Ring]) -> bool {
    let Some(outer) = polygon.first() else {
        return false;
    };
    if !point_in_ring(longitude, latitude, outer) {
        return false;
    }
    !polygon[1..]
        .iter()
        .any(|hole| point_in_ring(longitude, latitude, hole))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CountryProperties {
    pub name: String,
    pub code: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Feature {
    pub properties: Option<CountryProperties>,
    /// None for a feature with no usable geometry; a `Polygon` becomes one
    /// entry, a `MultiPolygon` its own list.
    pub polygons: Option<Vec<Polygon>>,
}

fn coord(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => fs_js::parse_number(s),
        Some(Value::Null) | None => f64::NAN,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(_) => f64::NAN,
    }
}

fn parse_polygon(v: &Value) -> Polygon {
    let Value::Array(rings) = v else {
        return Vec::new();
    };
    rings
        .iter()
        .map(|ring| match ring {
            Value::Array(points) => points
                .iter()
                .map(|p| [coord(p.get(0)), coord(p.get(1))])
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

impl Feature {
    pub(crate) fn from_value(v: &Value) -> Option<Feature> {
        if !v.is_object() {
            return None;
        }
        let properties = v
            .get("properties")
            .filter(|p| p.is_object())
            .map(|p| CountryProperties {
                name: p
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                code: p
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            });
        let polygons = v.get("geometry").filter(|g| g.is_object()).and_then(|g| {
            let coordinates = g.get("coordinates");
            if g.get("type").and_then(Value::as_str) == Some("Polygon") {
                return Some(vec![coordinates.map(parse_polygon).unwrap_or_default()]);
            }
            match coordinates {
                Some(Value::Array(polygons)) => Some(polygons.iter().map(parse_polygon).collect()),
                _ => None,
            }
        });
        Some(Feature {
            properties,
            polygons,
        })
    }
}

/// `countries.json`'s `features` array.
pub fn parse_features(text: &str) -> Vec<Feature> {
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    match doc.get("features") {
        Some(Value::Array(rows)) => rows.iter().filter_map(Feature::from_value).collect(),
        _ => Vec::new(),
    }
}

/// Properties of the first country whose polygon holds the point.
pub fn country_at(
    features: &[Feature],
    latitude: f64,
    longitude: f64,
) -> Option<&CountryProperties> {
    for feature in features {
        let Some(polygons) = &feature.polygons else {
            continue;
        };
        if polygons
            .iter()
            .any(|p| point_in_polygon(longitude, latitude, p))
        {
            return feature.properties.as_ref();
        }
    }
    None
}

/// The outer ring with each longitude unwrapped against the one before it, so
/// a ring crossing the antimeridian stays contiguous.
fn unwrap_ring(ring: &[[f64; 2]]) -> Ring {
    let mut points: Ring = Vec::new();
    let mut previous_longitude = ring[0][0];
    for p in ring {
        let mut longitude = p[0];
        let latitude = p[1];
        if !longitude.is_finite() || !latitude.is_finite() {
            continue;
        }
        if !points.is_empty() {
            while longitude - previous_longitude > 180.0 {
                longitude -= 360.0;
            }
            while longitude - previous_longitude < -180.0 {
                longitude += 360.0;
            }
        }
        points.push([longitude, latitude]);
        previous_longitude = longitude;
    }
    points
}

fn find_country<'a>(
    features: &'a [Feature],
    wanted: &str,
) -> Option<(&'a Feature, &'a Vec<Polygon>)> {
    for feature in features {
        let (Some(_), Some(polygons)) = (&feature.properties, &feature.polygons) else {
            // A feature without both is skipped before the code is read.
            continue;
        };
        if feature.properties.as_ref().unwrap().code.to_uppercase() != wanted {
            continue;
        }
        return Some((feature, polygons));
    }
    None
}

/// The area-weighted centroid of the country's largest polygon.
pub fn country_centre(features: &[Feature], code: &str) -> Option<LatLon> {
    let wanted = code.to_uppercase();
    let (_, polygons) = find_country(features, &wanted)?;

    struct Best {
        area: f64,
        latitude: f64,
        longitude: f64,
    }
    let mut best: Option<Best> = None;
    for polygon in polygons {
        let Some(ring) = polygon.first() else {
            continue;
        };
        if ring.len() < 3 {
            continue;
        }
        let points = unwrap_ring(ring);
        if points.len() < 3 {
            continue;
        }
        let (mut cross_sum, mut longitude_sum, mut latitude_sum) = (0.0, 0.0, 0.0);
        let mut previous = points.len() - 1;
        for index in 0..points.len() {
            let first = points[previous];
            let second = points[index];
            let cross = first[0] * second[1] - second[0] * first[1];
            cross_sum += cross;
            longitude_sum += (first[0] + second[0]) * cross;
            latitude_sum += (first[1] + second[1]) * cross;
            previous = index;
        }
        let area = cross_sum.abs();
        if area < 1e-9 || best.as_ref().is_some_and(|b| area <= b.area) {
            continue;
        }
        best = Some(Best {
            area,
            latitude: latitude_sum / (3.0 * cross_sum),
            longitude: wrap_longitude(longitude_sum / (3.0 * cross_sum)),
        });
    }
    best.map(|b| LatLon {
        latitude: b.latitude,
        longitude: b.longitude,
    })
}

/// Where a station with no coordinates of its own is drawn, memoised per
/// country and key.
#[derive(Debug, Default)]
pub struct EstimateCache(HashMap<String, LatLon>);

/// A point inside the country's largest polygon, picked by a generator seeded
/// from `key` (FNV-1a over its UTF-16 units, then the Numerical Recipes LCG),
/// so a station lands on the same spot every run. Falls back to the centroid
/// after 96 misses.
pub(crate) fn estimated_country_location(
    cache: &mut EstimateCache,
    features: &[Feature],
    code: &str,
    key: &str,
) -> Option<LatLon> {
    let wanted = code.to_uppercase();
    let source = if key.is_empty() { wanted.as_str() } else { key };
    let cache_key = format!("${wanted}:{source}");
    if let Some(hit) = cache.0.get(&cache_key) {
        return Some(*hit);
    }

    let mut best_ring: Option<Ring> = None;
    let mut best_area = 0.0;
    if let Some((_, polygons)) = find_country(features, &wanted) {
        for polygon in polygons {
            let Some(ring) = polygon.first() else {
                continue;
            };
            if ring.len() < 3 {
                continue;
            }
            let points = unwrap_ring(ring);
            if points.len() < 3 {
                continue;
            }
            let mut area = 0.0;
            let mut previous = points.len() - 1;
            for n in 0..points.len() {
                area += points[previous][0] * points[n][1] - points[n][0] * points[previous][1];
                previous = n;
            }
            let area = f64::abs(area);
            if area > best_area {
                best_area = area;
                best_ring = Some(points);
            }
        }
    }

    let ring = best_ring?;
    let (mut min_lon, mut max_lon) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_lat, mut max_lat) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &ring {
        min_lon = min_lon.min(p[0]);
        max_lon = max_lon.max(p[0]);
        min_lat = min_lat.min(p[1]);
        max_lat = max_lat.max(p[1]);
    }

    let mut seed: u32 = 2_166_136_261;
    for unit in source.encode_utf16() {
        seed ^= u32::from(unit);
        seed = seed.wrapping_mul(16_777_619);
    }
    let mut random = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(seed) / 4_294_967_296.0
    };

    for _ in 0..96 {
        let candidate_longitude = min_lon + random() * (max_lon - min_lon);
        let candidate_latitude = min_lat + random() * (max_lat - min_lat);
        if point_in_ring(candidate_longitude, candidate_latitude, &ring) {
            let location = LatLon {
                latitude: candidate_latitude,
                longitude: wrap_longitude(candidate_longitude),
            };
            cache.0.insert(cache_key, location);
            return Some(location);
        }
    }
    let centre = country_centre(features, &wanted);
    if let Some(c) = centre {
        cache.0.insert(cache_key, c);
    }
    centre
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// The globe pane: its size in pixels, the zoom, and the point looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub centre_latitude: f64,
    pub centre_longitude: f64,
}

pub fn station_position(station: &Station, view: View) -> Option<ScreenPosition> {
    let (latitude, longitude) = (station.latitude?, station.longitude?);
    let distance = view_distance(view.scale);
    let point = project_perspective(
        latitude,
        longitude,
        view.centre_latitude,
        view.centre_longitude,
        distance,
    );
    if !point.visible {
        return None;
    }
    let radius = view_radius(view.width, view.height, view.scale);
    Some(ScreenPosition {
        x: view.width / 2.0 + point.x * radius,
        y: view.height / 2.0 - point.y * radius,
        z: point.z,
    })
}

/// The frontmost visible station, preferring one that is not `excluded_uuid`.
/// With a positive finite `width`, `height` and `scale` only stations inside
/// that viewport count; pass NaN to skip the viewport test.
pub fn nearest_visible_station<'a>(
    stations: &'a [Station],
    centre_latitude: f64,
    centre_longitude: f64,
    excluded_uuid: &str,
    width: f64,
    height: f64,
    scale: f64,
) -> Option<&'a Station> {
    let constrain = width.is_finite()
        && width > 0.0
        && height.is_finite()
        && height > 0.0
        && scale.is_finite()
        && scale > 0.0;
    let distance = view_distance(if constrain { scale } else { 1.0 });
    let viewport_radius = if constrain {
        view_radius(width, height, scale)
    } else {
        0.0
    };
    let mut nearest: Option<&Station> = None;
    let mut nearest_depth = f64::NEG_INFINITY;
    let mut preferred: Option<&Station> = None;
    let mut preferred_depth = f64::NEG_INFINITY;

    for station in stations {
        let (Some(latitude), Some(longitude)) = (station.latitude, station.longitude) else {
            continue;
        };
        if !latitude.is_finite() || !longitude.is_finite() {
            continue;
        }
        let point = project_perspective(
            latitude,
            longitude,
            centre_latitude,
            centre_longitude,
            distance,
        );
        if !point.z.is_finite() || !point.visible {
            continue;
        }
        if constrain {
            let screen_x = width / 2.0 + point.x * viewport_radius;
            let screen_y = height / 2.0 - point.y * viewport_radius;
            if screen_x < 0.0 || screen_x > width || screen_y < 0.0 || screen_y > height {
                continue;
            }
        }
        if point.z > nearest_depth {
            nearest = Some(station);
            nearest_depth = point.z;
        }
        if station.uuid != excluded_uuid && point.z > preferred_depth {
            preferred = Some(station);
            preferred_depth = point.z;
        }
    }
    preferred.or(nearest)
}

pub fn merge_geo_stations(
    cache: &mut EstimateCache,
    primary: &[Station],
    secondary: &[Station],
    countries: &[Feature],
) -> Vec<Station> {
    let rows = merge_stations(primary, secondary, 5500);
    let mut output = Vec::new();
    for row in rows {
        if row.latitude.is_none() || row.longitude.is_none() {
            let Some(estimate) =
                estimated_country_location(cache, countries, &row.country_code, &row.uuid)
            else {
                continue;
            };
            output.push(Station {
                latitude: Some(estimate.latitude),
                longitude: Some(estimate.longitude),
                estimated_location: true,
                ..row
            });
        } else {
            output.push(row);
        }
        if output.len() >= 5500 {
            break;
        }
    }
    output
}

/// 0 `maximum` reads as 500, the default.
pub(crate) fn combine_stations(
    groups: &[&[Station]],
    maximum: usize,
    replace_duplicates: bool,
) -> Vec<Station> {
    let mut output: Vec<Station> = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let limit = (if maximum == 0 { 500 } else { maximum }).max(1);

    for (g, rows) in groups.iter().enumerate() {
        for row in *rows {
            if row.uuid.is_empty() {
                continue;
            }
            if let Some(&at) = seen.get(row.uuid.as_str()) {
                if replace_duplicates && g > 0 {
                    output[at] = row.clone();
                }
                continue;
            }
            if output.len() >= limit {
                continue;
            }
            seen.insert(&row.uuid, output.len());
            output.push(row.clone());
        }
    }
    output
}

pub fn merge_stations(primary: &[Station], secondary: &[Station], maximum: usize) -> Vec<Station> {
    combine_stations(&[primary, secondary], maximum, true)
}

pub fn prioritize_stations(
    priority: &[Station],
    fallback: &[Station],
    maximum: usize,
) -> Vec<Station> {
    combine_stations(&[priority, fallback], maximum, false)
}

/// Stations with `query` (case-folded) in the name, country, country code,
/// state, language, tags or codec. 0 `maximum` reads as 150.
pub fn search_stations(stations: &[Station], query: &str, maximum: usize) -> Vec<Station> {
    let wanted = query.trim().to_lowercase();
    if wanted.is_empty() {
        return Vec::new();
    }
    let limit = (if maximum == 0 { 150 } else { maximum }).max(1);
    let mut output = Vec::new();
    for station in stations {
        if output.len() >= limit {
            break;
        }
        let fields = [
            &station.name,
            &station.country,
            &station.country_code,
            &station.state,
            &station.language,
            &station.tags,
            &station.codec,
        ];
        if fields.iter().any(|f| f.to_lowercase().contains(&wanted)) {
            output.push(station.clone());
        }
    }
    output
}

/// 0 `maximum` reads as 150.
pub fn stations_for_country(stations: &[Station], code: &str, maximum: usize) -> Vec<Station> {
    let wanted = code.to_uppercase();
    if wanted.is_empty() {
        return Vec::new();
    }
    let limit = (if maximum == 0 { 150 } else { maximum }).max(1);
    let mut output = Vec::new();
    for station in stations {
        if output.len() >= limit {
            break;
        }
        if station.country_code.to_uppercase() == wanted {
            output.push(station.clone());
        }
    }
    output
}

/// A circular window of the list centred on `uuid`, so playback has a queue
/// either side of it. 0 `maximum` reads as 500.
pub fn station_window(stations: &[Station], uuid: &str, maximum: usize) -> Vec<Station> {
    let Some(index) = index_by_uuid(stations, uuid) else {
        return Vec::new();
    };
    let len = stations.len();
    let limit = len.min((if maximum == 0 { 500 } else { maximum }).max(1));
    let before = (limit - 1) / 2;
    let start = (index + len - before) % len;
    (0..limit)
        .map(|i| stations[(start + i) % len].clone())
        .collect()
}

pub fn station_meta(station: Option<&Station>) -> String {
    let Some(station) = station else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if station.uuid.starts_with("cliamp:") {
        parts.push("cliamp radio".into());
    }
    if !station.country_code.is_empty() {
        parts.push(station.country_code.clone());
    }
    if !station.codec.is_empty() {
        parts.push(station.codec.clone());
    }
    if station.bitrate > 0.0 {
        parts.push(format!("{} kbps", station.bitrate));
    }
    parts.join(" \u{b7} ")
}

pub fn index_by_uuid(stations: &[Station], uuid: &str) -> Option<usize> {
    stations.iter().position(|s| s.uuid == uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle_gap(a: f64, b: f64) -> f64 {
        let gap = (a - b).abs() % 360.0;
        if gap > 180.0 { 360.0 - gap } else { gap }
    }

    #[test]
    fn round_trip() {
        // (tag, lat, lon, lat0, lon0, d)
        let rows = [
            ("centre", 18.0, -20.0, 18.0, -20.0, 3.2),
            ("near", 40.4, -3.7, 30.0, 0.0, 3.2),
            ("south", -33.9, 151.2, -20.0, 140.0, 2.1),
            ("dateline", 10.0, 179.5, 5.0, -175.0, 3.2),
            ("closeUp", 52.37, 4.9, 52.0, 5.0, 1.05),
            ("nearHorizon", 0.0, 70.0, 0.0, 0.0, 3.2),
            ("polar", 75.0, 30.0, 70.0, -10.0, 4.06),
        ];
        for (tag, lat, lon, lat0, lon0, d) in rows {
            let p = project_perspective(lat, lon, lat0, lon0, d);
            assert!(p.visible, "{tag}: point should be on the near side");
            let back = unproject(p.x, p.y, lat0, lon0, d)
                .unwrap_or_else(|| panic!("{tag}: unproject missed the sphere"));
            assert!(
                (back.latitude - lat).abs() < 1e-6,
                "{tag}: latitude {} vs {lat}",
                back.latitude
            );
            assert!(
                angle_gap(back.longitude, lon) < 1e-6,
                "{tag}: longitude {} vs {lon}",
                back.longitude
            );
        }
    }

    #[test]
    fn past_horizon_rejected() {
        // cos 80 degrees is 0.17, under 1 / 3.2.
        assert!(!project_perspective(0.0, 80.0, 0.0, 0.0, 3.2).visible);
        assert!(!project_perspective(0.0, 180.0, 0.0, 0.0, 3.2).visible);
        // cos 71 degrees is 0.326, just over it.
        assert!(project_perspective(0.0, 71.0, 0.0, 0.0, 3.2).visible);
        let r = horizon_ratio(3.2) * 1.001;
        assert_eq!(unproject(r, 0.0, 0.0, 0.0, 3.2), None);
        assert_eq!(unproject(0.0, -r, 12.0, 40.0, 3.2), None);
        assert!(unproject(r * 0.99, 0.0, 0.0, 0.0, 3.2).is_some());
    }

    #[test]
    fn horizon_circle() {
        let d = 3.2;
        let c = (1.0_f64 / d).acos() * 180.0 / std::f64::consts::PI;
        let p = project_perspective(0.0, c, 0.0, 0.0, d);
        assert!(((p.x * p.x + p.y * p.y).sqrt() - horizon_ratio(d)).abs() < 1e-9);
    }

    #[test]
    fn zoom_clamp() {
        assert_eq!(view_distance(1.0), 3.2);
        assert!(view_distance(1000.0) >= 1.05);
        assert!(view_distance(24.0) < view_distance(1.0));
    }

    #[test]
    fn kinetic_rotation_decays_and_stops() {
        let state = KineticState {
            longitude: 0.0,
            latitude: 0.0,
            velocity_x: 100.0,
            velocity_y: 0.0,
        };
        let options = KineticOptions {
            deceleration: Some(50.0),
            scale: Some(1.0),
            longitude_sensitivity: Some(0.1),
            latitude_sensitivity: Some(0.1),
            ..KineticOptions::default()
        };
        let step = advance_kinetic_rotation(state, 3.0, options);
        assert!(!step.active);
        assert_eq!(step.velocity_x, 0.0);
        assert!(step.longitude < 0.0);
        let launch = kinetic_launch_velocity(3.0, 4.0, 10.0, 100.0);
        assert!(!launch.active);
        assert_eq!(limit_kinetic_velocity(300.0, 400.0, 100.0).speed, 100.0);
    }

    fn square(code: &str, lon: f64, lat: f64, size: f64) -> Feature {
        Feature {
            properties: Some(CountryProperties {
                name: code.into(),
                code: code.into(),
            }),
            polygons: Some(vec![vec![vec![
                [lon, lat],
                [lon + size, lat],
                [lon + size, lat + size],
                [lon, lat + size],
                [lon, lat],
            ]]]),
        }
    }

    #[test]
    fn country_lookups_and_estimates() {
        let features = [square("AA", 10.0, 10.0, 10.0)];
        assert_eq!(country_at(&features, 15.0, 15.0).unwrap().code, "AA");
        assert!(country_at(&features, 25.0, 15.0).is_none());
        let centre = country_centre(&features, "aa").unwrap();
        assert!((centre.latitude - 15.0).abs() < 1e-9 && (centre.longitude - 15.0).abs() < 1e-9);
        let mut cache = EstimateCache::default();
        let one = estimated_country_location(&mut cache, &features, "AA", "station-1").unwrap();
        let again =
            estimated_country_location(&mut EstimateCache::default(), &features, "AA", "station-1")
                .unwrap();
        assert_eq!(one, again);
        assert!(point_in_polygon(
            one.longitude,
            one.latitude,
            features[0].polygons.as_ref().unwrap().first().unwrap()
        ));
        let geo = merge_geo_stations(
            &mut cache,
            &[Station {
                uuid: "u".into(),
                country_code: "AA".into(),
                ..Station::default()
            }],
            &[],
            &features,
        );
        assert!(geo[0].estimated_location && geo[0].latitude.is_some());
    }
}
