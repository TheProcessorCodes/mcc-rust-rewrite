//! A bike's physics values: the BXML asset `<class>_vehicle` (`mx450f_vehicle`
//! for the Dakumoto 450F) in `Database\EngineDependencies.dx9.package`.
//!
//! Its root is `<vehicle_physics>`, with one child per value (`Engine_TorqueCurve`,
//! `MassProps_MainBodyMass`, ...). Every value is text: a number, a vector
//! `"0.0,-32.2,0"`, or a curve `"x0,y0,x1,y1,..."` with the attributes
//! `num_knots`, `mleft` and `mright`. The tyre friction and drag values come
//! three times, with `preset` `Sticky`, `Normal` and `Slippery`; the rest once,
//! `Normal`.
//!
//! [`xml`] gives the asset as the readable XML the `reflex_core` simulation
//! loads (`Vehicle::from_xml_str`): `kind="scalar" | "vec3" | "curve"`, curves
//! as `<knot x y/>` children.

use std::fmt::Write;
use std::path::Path;

use crate::Error;
use crate::bxml::Bxml;
use crate::package::Package;

/// The package holding every vehicle's values.
const PACKAGE: &str = "EngineDependencies";

/// The BXML document of `<class>_vehicle` (`class` e.g. `mx450f`).
pub fn read(root: &Path, class: &str) -> Result<Bxml, Error> {
    let package = Package::open(&root.join("Database"), PACKAGE)?;
    let name = format!("{class}_vehicle");
    let asset = package
        .assets
        .iter()
        .find(|a| a.name == name && a.kind == "bxml")
        .ok_or_else(|| Error::Missing(format!("{PACKAGE}/{name}.bxml")))?;
    let doc = Bxml::parse(&package.read(asset)?)?;
    if doc.name(0) != "vehicle_physics" {
        return Err(Error::Format(format!("{name}: root is <{}>, not <vehicle_physics>", doc.name(0))));
    }
    Ok(doc)
}

/// `<class>_vehicle` as XML text (see the module notes).
pub fn xml(root: &Path, class: &str) -> Result<String, Error> {
    to_xml(&read(root, class)?)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A `<vehicle_physics>` document as XML text.
pub fn to_xml(doc: &Bxml) -> Result<String, Error> {
    let mut out = String::from("<vehicle_physics>\n");
    for node in doc.children(0) {
        let key = doc.name(node);
        let text = doc.text(node).unwrap_or("").trim();
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        let preset = doc.attr_str(node, "preset").unwrap_or("Normal");
        let _ = write!(out, "  <{key} preset=\"{}\"", escape(preset));
        if doc.attr_str(node, "num_knots").is_some() {
            if parts.len() % 2 != 0 {
                return Err(Error::Format(format!("{key}: curve has {} numbers", parts.len())));
            }
            let slope = |k: &str| doc.attr_str(node, k).unwrap_or("0").trim().to_string();
            let _ = writeln!(
                out,
                " kind=\"curve\" mleft=\"{}\" mright=\"{}\">",
                escape(&slope("mleft")),
                escape(&slope("mright"))
            );
            for knot in parts.chunks(2) {
                let _ = writeln!(out, "    <knot x=\"{}\" y=\"{}\" />", escape(knot[0]), escape(knot[1]));
            }
            let _ = writeln!(out, "  </{key}>");
        } else {
            let kind = if parts.len() == 1 { "scalar" } else { "vec3" };
            let _ = writeln!(out, " kind=\"{kind}\">{}</{key}>", escape(text));
        }
    }
    out.push_str("</vehicle_physics>\n");
    Ok(out)
}
