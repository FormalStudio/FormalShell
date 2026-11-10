// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! Natural Earth's countries (`data/countries.json`): their outlines for
//! the globe and their polygons for Radio Atlas's lookups.

use std::f64::consts::PI;

use serde_json::Value;

const COUNTRIES: &str = include_str!("../../data/countries.json");

/// A ring is `[longitude, latitude]` pairs; a coordinate JS read as NaN stays
/// NaN.
pub(crate) type Ring = Vec<[f64; 2]>;
/// An outer ring followed by holes.
pub(crate) type Polygon = Vec<Ring>;

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
    fn from_value(v: &Value) -> Option<Feature> {
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
fn parse_features(text: &str) -> Vec<Feature> {
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    match doc.get("features") {
        Some(Value::Array(rows)) => rows.iter().filter_map(Feature::from_value).collect(),
        _ => Vec::new(),
    }
}

pub struct UnitRing {
    pub world: Vec<[f64; 3]>,
    pub centroid: [f64; 3],
}

pub struct Outline {
    pub code: String,
    pub rings: Vec<UnitRing>,
}

/// countries.json parsed and turned into unit vectors, done once, off the
/// UI thread.
pub struct Countries {
    pub features: Vec<Feature>,
    pub outlines: Vec<Outline>,
}

impl std::fmt::Debug for Countries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Countries({})", self.features.len())
    }
}

/// The point on the unit sphere at a latitude and longitude in degrees.
pub fn unit(latitude: f64, longitude: f64) -> Option<[f64; 3]> {
    let (lat, lon) = (latitude * PI / 180.0, longitude * PI / 180.0);
    if !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    Some([lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()])
}

pub fn load() -> Countries {
    let features = parse_features(COUNTRIES);
    let mut outlines = Vec::new();
    for f in &features {
        let Some(polygons) = &f.polygons else { continue };
        let mut rings = Vec::new();
        for polygon in polygons {
            let Some(ring) = polygon.first() else { continue };
            let world: Vec<[f64; 3]> = ring.iter().filter_map(|p| unit(p[1], p[0])).collect();
            if world.len() < 3 {
                continue;
            }
            let c = world.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            let length = if length == 0.0 { 1.0 } else { length };
            rings.push(UnitRing { world, centroid: [c[0] / length, c[1] / length, c[2] / length] });
        }
        if !rings.is_empty() {
            let code = f.properties.as_ref().map(|p| p.code.to_uppercase()).unwrap_or_default();
            outlines.push(Outline { code, rings });
        }
    }
    Countries { features, outlines }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outlines_load() {
        let c = load();
        assert!(c.features.len() > 150);
        assert!(c.outlines.iter().any(|o| o.code == "FR"));
    }
}
