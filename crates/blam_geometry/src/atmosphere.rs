//! The scenario's atmosphere: the `skya` (sky_atm_parameters) tag the
//! scenario points at, whose settings drive Halo 3's light-scattering fog
//! and give the sky its colour. Follows `docs/format/sky-and-atmosphere.md`.

use blam_cache::CacheFile;
use glam::Vec3;

use crate::{Error, Result};

/// Where the scenario keeps its `skya` reference (MCC U13).
const SCNR_SKY_PARAMETERS: usize = 0x680;
/// The atmosphere settings block inside `skya`.
const SKYA_SETTINGS: usize = 0x34;
/// One atmosphere setting.
const SETTING_SIZE: usize = 0xA4;

/// One named atmosphere setting (an entry of the `skya` settings block).
///
/// Scattering coefficients are per world unit. `beta_m` is the molecular
/// (Rayleigh) term, `beta_p` the particle (Mie, haze) term; the `*_theta`
/// triples are the same coefficients times their phase-function prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct AtmosphereSetting {
    pub name: String,
    pub flags: u16,
    /// Sun elevation above the horizon, degrees.
    pub sun_elevation: f32,
    /// Sun heading about +Z from +X, degrees.
    pub sun_heading: f32,
    /// Sun colour (linear) and intensity: the light being scattered.
    pub sun_colour: [f32; 3],
    pub sun_intensity: f32,
    /// Height (world units) at which both densities are 1.
    pub sea_level: f32,
    /// Heights over which the molecular / particle densities fall by e.
    pub rayleigh_height: f32,
    pub mie_height: f32,
    /// Scales that the stored coefficients already include.
    pub rayleigh_multiplier: f32,
    pub mie_multiplier: f32,
    /// Henyey-Greenstein asymmetry of the particle phase function.
    pub mie_g: f32,
    pub desaturation: f32,
    /// Added to the view distance (world units) before fogging.
    pub distance_bias: f32,
    /// The longest distance (world units) that is fogged.
    pub max_fog_thickness: f32,
    pub beta_m: [f32; 3],
    pub beta_p: [f32; 3],
    pub beta_m_theta: [f32; 3],
    pub beta_p_theta: [f32; 3],
}

impl AtmosphereSetting {
    /// Unit vector toward the sun, Halo frame (Z up).
    pub fn sun_direction(&self) -> Vec3 {
        let (e, h) = (self.sun_elevation.to_radians(), self.sun_heading.to_radians());
        Vec3::new(e.cos() * h.cos(), e.cos() * h.sin(), e.sin())
    }

    /// Whether the setting is meant for the sky dome (flag bit 2; riverworld's
    /// `haze_skydome` entries have it, `haze_level` does not).
    pub fn for_sky(&self) -> bool {
        self.flags & 4 != 0
    }
}

/// The scenario's `skya` tag, decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Atmosphere {
    pub tag_name: String,
    pub settings: Vec<AtmosphereSetting>,
}

impl Atmosphere {
    /// The setting that fogs the world: the first one not marked for the sky.
    pub fn world(&self) -> Option<&AtmosphereSetting> {
        self.settings.iter().find(|s| !s.for_sky()).or(self.settings.first())
    }

    /// The setting that colours the sky: the first one marked for the sky,
    /// else the world's.
    pub fn sky(&self) -> Option<&AtmosphereSetting> {
        self.settings.iter().find(|s| s.for_sky()).or_else(|| self.world())
    }
}

fn f32x3(r: &mut blam_cache::Reader<'_>) -> Result<[f32; 3]> {
    Ok(r.f32x3()?)
}

/// Reads the scenario's atmosphere; `None` when it has no `skya` reference.
pub fn read(cache: &CacheFile) -> Result<Option<Atmosphere>> {
    let scnr = cache
        .scenario()
        .ok_or_else(|| Error::Invalid("no scenario tag".into()))?;
    let at = cache.tag_offset(scnr)? + SCNR_SKY_PARAMETERS;
    let reference = cache.reader_at(at).tag_ref()?;
    if reference.class.as_str() != "skya" {
        return Ok(None);
    }
    let Some(tag) = reference.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(None);
    };
    let base = cache.tag_offset(tag)?;
    let block = cache.reader_at(base + SKYA_SETTINGS).block()?;
    if block.count > 64 {
        return Err(Error::Invalid(format!(
            "{}: {} atmosphere settings",
            tag.name(),
            block.count
        )));
    }
    let mut settings = Vec::new();
    for o in block.elements(SETTING_SIZE) {
        let mut r = cache.reader_at(o);
        let flags = r.u16()?;
        r.skip(2);
        let name = cache.string(r.string_id()?).unwrap_or("").to_string();
        settings.push(AtmosphereSetting {
            name,
            flags,
            sun_elevation: r.f32()?,
            sun_heading: r.f32()?,
            sun_colour: f32x3(&mut r)?,
            sun_intensity: r.f32()?,
            sea_level: r.f32()?,
            rayleigh_height: r.f32()?,
            mie_height: r.f32()?,
            rayleigh_multiplier: r.f32()?,
            mie_multiplier: r.f32()?,
            mie_g: r.f32()?,
            desaturation: r.f32()?,
            distance_bias: r.f32()?,
            max_fog_thickness: r.f32()?,
            beta_m: f32x3(&mut r)?,
            beta_p: f32x3(&mut r)?,
            beta_m_theta: f32x3(&mut r)?,
            beta_p_theta: f32x3(&mut r)?,
        });
    }
    Ok(Some(Atmosphere {
        tag_name: tag.name().to_string(),
        settings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str) -> Option<CacheFile> {
        let dir = std::env::var_os("HALO3_MAPS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps".into()
            });
        let path = dir.join(format!("{name}.map"));
        path.exists().then(|| CacheFile::open(path).unwrap())
    }

    #[test]
    fn halo_atmosphere_settings() {
        // 120_halo's clusters pick these through their BSP's atmosphere
        // palette (`sbsp-portals.md` 5).
        let Some(cache) = open("120_halo") else { return };
        let a = read(&cache).unwrap().unwrap();
        let names: Vec<&str> = a.settings.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "haze_used_for_halo_zig_sky",
                "snow_light",
                "snow_heavy",
                "haze",
                "interior",
                "trench",
                "zigg_activated",
                "space"
            ]
        );
    }

    #[test]
    fn riverworld_atmosphere() {
        let Some(cache) = open("riverworld") else { return };
        let a = read(&cache).unwrap().unwrap();
        assert_eq!(a.tag_name, r"levels\multi\riverworld\sky\riverworld");
        let names: Vec<&str> = a.settings.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["haze_level", "haze_skydome", "not_used", "haze_skydome"]);
        let w = a.world().unwrap();
        assert_eq!(w.name, "haze_level");
        assert_eq!(a.sky().unwrap().name, "haze_skydome");
        assert_eq!((w.sun_elevation, w.sun_heading), (45.0, 25.0));
        assert_eq!((w.rayleigh_height, w.mie_height, w.max_fog_thickness), (150.0, 6.0, 65536.0));
        // The stored coefficients are the multipliers times one base set:
        // the sky's Rayleigh multiplier is 2.8x the level's, and so are its
        // coefficients.
        let s = a.sky().unwrap();
        for c in 0..3 {
            let k = s.beta_m[c] / w.beta_m[c];
            assert!((k - s.rayleigh_multiplier / w.rayleigh_multiplier).abs() < 1e-3, "{k}");
            let k = s.beta_p[c] / w.beta_p[c];
            assert!((k - s.mie_multiplier / w.mie_multiplier).abs() < 1e-3, "{k}");
            // Rayleigh phase prefix: one constant for every channel.
            let p = w.beta_m_theta[c] / w.beta_m[c];
            assert!((p - 0.0607).abs() < 1e-3, "{p}");
        }
        // Rayleigh scatters blue most, haze much less selectively.
        assert!(w.beta_m[2] / w.beta_m[0] > 3.0 && w.beta_p[2] / w.beta_p[0] < 2.0);
    }

    /// The sun drawn in the sky model sits where the atmosphere's sun angles
    /// point: the first angle is the elevation, the second the heading.
    #[test]
    fn riverworld_sundisk_matches_sun_angles() {
        let Some(cache) = open("riverworld") else { return };
        let mut scene = crate::Scene::default();
        let options = crate::Options {
            collision: false,
            ..Default::default()
        };
        crate::decode_into(&cache, &options, &mut scene).unwrap();
        let mut sum = Vec3::ZERO;
        let mut n = 0;
        for p in scene.placements.iter().filter(|p| p.kind == crate::Kind::Sky) {
            let mesh = &scene.meshes[p.mesh as usize];
            for part in &mesh.parts {
                let m = &scene.materials[part.material as usize];
                if !m.tag_path.ends_with(r"\sundisk") {
                    continue;
                }
                for &i in &part.indices {
                    sum += p.transform.transform_point3(mesh.positions[i as usize]);
                    n += 1;
                }
            }
        }
        assert!(n > 0, "no sundisk in the sky model");
        let disk = (sum / n as f32).normalize();
        let sun = read(&cache).unwrap().unwrap().world().unwrap().sun_direction();
        let angle = disk.dot(sun).clamp(-1.0, 1.0).acos().to_degrees();
        eprintln!("sundisk direction {disk:?}, atmosphere sun {sun:?}: {angle:.1} deg apart");
        assert!(angle < 5.0, "{angle} degrees apart");
    }

    #[test]
    fn cyberdyne_atmosphere() {
        let Some(cache) = open("cyberdyne") else { return };
        let a = read(&cache).unwrap().unwrap();
        assert_eq!(a.settings.len(), 1);
        let s = &a.settings[0];
        assert_eq!(s.name, "cyberdyne_sky_fog");
        assert!((s.mie_g - 0.6).abs() < 1e-6 && (s.distance_bias + 25.0).abs() < 1e-6);
    }
}
