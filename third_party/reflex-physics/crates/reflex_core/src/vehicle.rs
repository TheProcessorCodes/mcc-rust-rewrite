//! Vehicle tuning values (spec 2.1) and their loaders.
//!
//! Three source formats, all from the user's own install (nothing is bundled):
//! - stock JSON dump (`dumps\stock-json\*.json`): `"Key@Preset": value`
//! - stock XML dump (`dumps\stock\*.xml`): `<vehicle_physics>` children with `preset` and `kind`
//! - in-memory tuning (`runs\...\tuning-*.json`): `"Key": {"type": 0..3, "value": ...}`
//!
//! Keys are kept by name; nothing is dropped. A key that is not present reads as the sentinel
//! [`SENTINEL`], as in the original.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde_json::Value as Json;

use crate::curve::{blend3, Curve};

/// Value the original reads for a key that the vehicle file does not define.
pub const SENTINEL: f64 = -777.7;

/// One tuning value. `Triple` is a per-surface key: (Sticky, Normal, Slippery).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Scalar(f64),
    Vec3([f64; 3]),
    Curve(Curve),
    Triple([f64; 3]),
}

#[derive(Debug)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

fn err<T>(msg: impl Into<String>) -> Result<T, LoadError> {
    Err(LoadError(msg.into()))
}

/// The ~108 values of one vehicle, by key name (`Engine_TorqueCurve`, ...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vehicle {
    pub values: BTreeMap<String, Value>,
}

impl Vehicle {
    /// Loads any of the three formats, chosen by extension and content.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| LoadError(format!("{}: {e}", path.display())))?;
        let is_xml = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
            || text.trim_start().starts_with('<');
        if is_xml {
            Self::from_xml_str(&text)
        } else {
            Self::from_json_str(&text)
        }
        .map_err(|e| LoadError(format!("{}: {e}", path.display())))
    }

    /// Stock JSON dump or tuning JSON (detected per file).
    pub fn from_json_str(text: &str) -> Result<Self, LoadError> {
        let json: Json = serde_json::from_str(text).map_err(|e| LoadError(e.to_string()))?;
        let Json::Object(map) = json else { return err("top level is not an object") };
        let is_tuning = map
            .iter()
            .find(|(k, _)| !k.starts_with('_'))
            .is_some_and(|(k, v)| !k.contains('@') && v.get("type").is_some());
        if is_tuning {
            Self::from_tuning(&map)
        } else {
            Self::from_stock_json(&map)
        }
    }

    fn from_tuning(map: &serde_json::Map<String, Json>) -> Result<Self, LoadError> {
        let mut values = BTreeMap::new();
        for (key, entry) in map {
            if key.starts_with('_') {
                continue;
            }
            let ty = entry.get("type").and_then(Json::as_u64);
            let v = entry.get("value").ok_or_else(|| LoadError(format!("{key}: no value")))?;
            let value = match ty {
                Some(0) => Value::Scalar(num(v, key)?),
                // vec3 keys are stored as vec4 in memory, w unused
                Some(1) => Value::Vec3(vec3(v, key)?),
                Some(2) => Value::Curve(json_curve(v, key)?),
                Some(3) => Value::Triple(vec3(v, key)?),
                _ => return err(format!("{key}: unknown type {ty:?}")),
            };
            values.insert(key.clone(), value);
        }
        Ok(Self { values })
    }

    fn from_stock_json(map: &serde_json::Map<String, Json>) -> Result<Self, LoadError> {
        let mut values = BTreeMap::new();
        let mut presets: BTreeMap<String, [Option<f64>; 3]> = BTreeMap::new();
        for (full, v) in map {
            if full.starts_with('_') {
                continue;
            }
            let (key, preset) = full.split_once('@').unwrap_or((full, "Normal"));
            if preset != "Normal" || presets.contains_key(key) {
                let slot = preset_index(preset).ok_or_else(|| LoadError(format!("{full}: preset")))?;
                presets.entry(key.to_string()).or_default()[slot] = Some(num(v, full)?);
                continue;
            }
            let value = match v {
                Json::Number(_) => Value::Scalar(num(v, full)?),
                Json::Array(_) => Value::Vec3(vec3(v, full)?),
                Json::Object(_) => Value::Curve(json_curve(v, full)?),
                _ => return err(format!("{full}: unexpected value")),
            };
            values.insert(key.to_string(), value);
        }
        // Keys with a Sticky / Slippery element are per-surface triples; the Normal element was
        // stored as a scalar above if it came first.
        for (key, mut p) in presets {
            if p[1].is_none() {
                if let Some(Value::Scalar(n)) = values.get(&key) {
                    p[1] = Some(*n);
                }
            }
            let [Some(a), Some(b), Some(c)] = p else {
                return err(format!("{key}: incomplete Sticky/Normal/Slippery set"));
            };
            values.insert(key, Value::Triple([a, b, c]));
        }
        Ok(Self { values })
    }

    /// Stock XML dump (`vehicle_bxml.py` format).
    pub fn from_xml_str(text: &str) -> Result<Self, LoadError> {
        let doc = roxmltree::Document::parse(text).map_err(|e| LoadError(e.to_string()))?;
        let vp = doc
            .descendants()
            .find(|n| n.has_tag_name("vehicle_physics"))
            .ok_or_else(|| LoadError("no <vehicle_physics>".into()))?;
        let mut values = BTreeMap::new();
        let mut presets: BTreeMap<String, [Option<f64>; 3]> = BTreeMap::new();
        for el in vp.children().filter(|n| n.is_element()) {
            let key = el.tag_name().name();
            let preset = el.attribute("preset").unwrap_or("Normal");
            let kind = el.attribute("kind").unwrap_or("scalar");
            let text = el.text().unwrap_or("").trim();
            let value = match kind {
                "scalar" => Value::Scalar(parse_f(text, key)?),
                "vec3" => {
                    let parts: Vec<f64> =
                        text.split(',').map(|s| parse_f(s, key)).collect::<Result<_, _>>()?;
                    if parts.len() < 3 {
                        return err(format!("{key}: vec3 has {} parts", parts.len()));
                    }
                    Value::Vec3([parts[0], parts[1], parts[2]])
                }
                "curve" => {
                    let knots = el
                        .children()
                        .filter(|n| n.has_tag_name("knot"))
                        .map(|k| Ok((attr_f(k, "x", key)?, attr_f(k, "y", key)?)))
                        .collect::<Result<Vec<_>, LoadError>>()?;
                    Value::Curve(Curve::new(&knots, attr_f(el, "mleft", key)?, attr_f(el, "mright", key)?))
                }
                _ => return err(format!("{key}: unknown kind {kind}")),
            };
            let slot = preset_index(preset).ok_or_else(|| LoadError(format!("{key}: preset {preset}")))?;
            match value {
                Value::Scalar(s) => {
                    presets.entry(key.to_string()).or_default()[slot] = Some(s);
                }
                other if slot == 1 => {
                    values.insert(key.to_string(), other);
                }
                _ => return err(format!("{key}: non-scalar {preset} preset")),
            }
        }
        for (key, p) in presets {
            let value = match p {
                [None, Some(n), None] => Value::Scalar(n),
                [Some(a), Some(b), Some(c)] => Value::Triple([a, b, c]),
                _ => return err(format!("{key}: incomplete Sticky/Normal/Slippery set")),
            };
            values.insert(key, value);
        }
        Ok(Self { values })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Scalar key; a missing key reads as [`SENTINEL`]. A triple reads as its Normal element.
    pub fn scalar(&self, key: &str) -> f64 {
        match self.values.get(key) {
            Some(Value::Scalar(v)) => *v,
            Some(Value::Triple(t)) => t[1],
            _ => SENTINEL,
        }
    }

    /// vec3 key; a missing key reads as [`SENTINEL`] in every component.
    pub fn vec3(&self, key: &str) -> [f64; 3] {
        match self.values.get(key) {
            Some(Value::Vec3(v)) => *v,
            _ => [SENTINEL; 3],
        }
    }

    pub fn curve(&self, key: &str) -> Option<&Curve> {
        match self.values.get(key) {
            Some(Value::Curve(c)) => Some(c),
            _ => None,
        }
    }

    /// Per-surface key evaluated at grip / drag index `s` with [`blend3`]. A plain scalar is constant.
    pub fn surface(&self, key: &str, s: f64) -> f64 {
        match self.values.get(key) {
            Some(Value::Triple(t)) => blend3(t, s),
            Some(Value::Scalar(v)) => *v,
            _ => SENTINEL,
        }
    }

    /// Evaluates a curve key at `x`; a missing curve reads as [`SENTINEL`].
    pub fn eval(&self, key: &str, x: f64) -> f64 {
        self.curve(key).map_or(SENTINEL, |c| c.eval(x))
    }
}

fn preset_index(p: &str) -> Option<usize> {
    match p {
        "Sticky" => Some(0),
        "Normal" => Some(1),
        "Slippery" => Some(2),
        _ => None,
    }
}

fn num(v: &Json, key: &str) -> Result<f64, LoadError> {
    v.as_f64().ok_or_else(|| LoadError(format!("{key}: not a number")))
}

fn vec3(v: &Json, key: &str) -> Result<[f64; 3], LoadError> {
    let a = v.as_array().ok_or_else(|| LoadError(format!("{key}: not an array")))?;
    if a.len() < 3 {
        return err(format!("{key}: array has {} elements", a.len()));
    }
    Ok([num(&a[0], key)?, num(&a[1], key)?, num(&a[2], key)?])
}

fn json_curve(v: &Json, key: &str) -> Result<Curve, LoadError> {
    let knots = v
        .get("knots")
        .and_then(Json::as_array)
        .ok_or_else(|| LoadError(format!("{key}: no knots")))?
        .iter()
        .map(|k| {
            let [x, y, ..] = vec_any(k, key)?[..] else { return err(format!("{key}: bad knot")) };
            Ok((x, y))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let m = |name: &str| v.get(name).map_or(err(format!("{key}: no {name}")), |x| num(x, key));
    Ok(Curve::new(&knots, m("mleft")?, m("mright")?))
}

fn vec_any(v: &Json, key: &str) -> Result<Vec<f64>, LoadError> {
    v.as_array()
        .ok_or_else(|| LoadError(format!("{key}: not an array")))?
        .iter()
        .map(|x| num(x, key))
        .collect()
}

fn parse_f(s: &str, key: &str) -> Result<f64, LoadError> {
    s.trim().parse().map_err(|_| LoadError(format!("{key}: bad number {s:?}")))
}

fn attr_f(n: roxmltree::Node, attr: &str, key: &str) -> Result<f64, LoadError> {
    parse_f(n.attribute(attr).ok_or_else(|| LoadError(format!("{key}: no {attr}")))?, key)
}
