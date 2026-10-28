//! What the scenario says about loading and starting a level: zone sets
//! (which structure BSPs are loaded together), player starting locations,
//! trigger volumes and the zone-set switch volumes that move between zone
//! sets. Follows `docs/format/zone-sets.md`.

use glam::Vec3;

use blam_cache::{Block, CacheFile};

use crate::scene::{ClusterInfo, Scene, StartingLocation, TriggerVolume, ZoneSet, ZoneSwitch};
use crate::{Error, Result};

/// Offsets in the scenario (MCC U13), see `zone-sets.md` section 1.
const ZONE_SETS: usize = 0x58;
const ZONE_SET_SIZE: usize = 40;
const STARTING_LOCATIONS: usize = 0x24C;
const STARTING_LOCATION_SIZE: usize = 24;
const TRIGGER_VOLUMES: usize = 0x258;
const TRIGGER_VOLUME_SIZE: usize = 68;
const ZONE_SWITCHES: usize = 0x270;
const ZONE_SWITCH_SIZE: usize = 8;

/// Reads zone sets, starting locations, trigger volumes and zone-set switch
/// volumes into `scene`.
pub fn decode(cache: &CacheFile, scene: &mut Scene) -> Result<()> {
    let scnr = cache.scenario().ok_or_else(|| {
        Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path))
    })?;
    let s = cache.tag_offset(scnr)?;
    let name_of = |id| {
        cache
            .string(id)
            .map_or_else(String::new, str::to_string)
    };

    let block: Block = cache.reader_at(s + ZONE_SETS).block()?;
    for o in block.elements(ZONE_SET_SIZE) {
        let mut r = cache.reader_at(o);
        let name = name_of(r.string_id()?);
        let bsps = r.at(o + 0x0C).u32()?;
        scene.zone_sets.push(ZoneSet { name, bsps });
    }

    let block: Block = cache.reader_at(s + STARTING_LOCATIONS).block()?;
    for o in block.elements(STARTING_LOCATION_SIZE) {
        let mut r = cache.reader_at(o);
        let position = Vec3::from(r.f32x3()?);
        let [yaw, pitch] = r.f32x2()?;
        scene.starts.push(StartingLocation {
            position,
            yaw,
            pitch,
        });
    }

    let block: Block = cache.reader_at(s + TRIGGER_VOLUMES).block()?;
    for o in block.elements(TRIGGER_VOLUME_SIZE) {
        let mut r = cache.reader_at(o);
        let name = name_of(r.string_id()?);
        let mut r = r.at(o + 0x0C);
        let forward = Vec3::from(r.f32x3()?);
        let up = Vec3::from(r.f32x3()?);
        let position = Vec3::from(r.f32x3()?);
        let extents = Vec3::from(r.f32x3()?);
        scene.trigger_volumes.push(TriggerVolume {
            name,
            forward,
            up,
            position,
            extents,
        });
    }

    let block: Block = cache.reader_at(s + ZONE_SWITCHES).block()?;
    let index = |v: i16| u32::try_from(v).ok();
    for o in block.elements(ZONE_SWITCH_SIZE) {
        let mut r = cache.reader_at(o);
        let flags = r.u16()?;
        let begin = index(r.i16()?);
        let volume = index(r.i16()?);
        let commit = index(r.i16()?);
        let Some(volume) = volume else { continue };
        scene.zone_switches.push(ZoneSwitch {
            flags,
            begin,
            volume,
            commit,
        });
    }
    Ok(())
}

impl Scene {
    /// The zone set to load first: the one the game starts in (the first),
    /// unless a starting location lies outside its BSPs and another zone set
    /// holds it. Zero BSPs when the scenario lists no zone sets.
    pub fn start_zone_set(&self) -> Option<usize> {
        if self.zone_sets.is_empty() {
            return None;
        }
        let Some(start) = self.starts.first() else {
            return Some(0);
        };
        let holds = |z: &ZoneSet| {
            self.bsps.iter().any(|b| {
                z.bsps & (1 << b.index) != 0
                    && start.position.cmpge(b.bounds.0).all()
                    && start.position.cmple(b.bounds.1).all()
            })
        };
        if holds(&self.zone_sets[0]) {
            return Some(0);
        }
        Some(self.zone_sets.iter().position(holds).unwrap_or(0))
    }

    /// The sky most clusters of the loaded BSPs (`mask`) look out at, among
    /// the skies placed in `skies`; the first of those active in a loaded
    /// BSP when no cluster names one.
    pub fn main_sky(&self, mask: u32, skies: &[(u32, u32)]) -> Option<u32> {
        let placed = |s: u32| skies.iter().any(|(i, _)| *i == s);
        let mut votes: Vec<(u32, usize)> = Vec::new();
        for b in self.bsps.iter().filter(|b| mask & (1 << b.index) != 0) {
            for s in b.clusters.iter().filter_map(|c| c.sky).filter(|s| placed(*s)) {
                match votes.iter_mut().find(|(i, _)| *i == s) {
                    Some(v) => v.1 += 1,
                    None => votes.push((s, 1)),
                }
            }
        }
        votes
            .iter()
            .max_by_key(|(i, n)| (*n, std::cmp::Reverse(*i)))
            .map(|(i, _)| *i)
            .or_else(|| {
                skies
                    .iter()
                    .find(|(_, active)| *active == 0 || active & mask != 0)
                    .map(|(i, _)| *i)
            })
    }

    /// The sky seen from the smallest loaded cluster around `p`, if that
    /// cluster names one (indoors it does not).
    pub fn sky_at(&self, p: Vec3, mask: u32) -> Option<u32> {
        self.cluster_at(p, mask).and_then(|c| c.sky)
    }

    /// The atmosphere setting most clusters of the loaded BSPs (`mask`)
    /// name, for a camera outside every cluster box.
    pub fn main_atmosphere(&self, mask: u32) -> Option<u32> {
        let mut votes: Vec<(u32, usize)> = Vec::new();
        for b in self.bsps.iter().filter(|b| mask & (1 << b.index) != 0) {
            for a in b.clusters.iter().filter_map(|c| c.atmosphere) {
                match votes.iter_mut().find(|(i, _)| *i == a) {
                    Some(v) => v.1 += 1,
                    None => votes.push((a, 1)),
                }
            }
        }
        votes
            .iter()
            .max_by_key(|(i, n)| (*n, std::cmp::Reverse(*i)))
            .map(|(i, _)| *i)
    }

    /// The smallest cluster of a loaded BSP (`mask`) whose bounds hold `p`.
    pub fn cluster_at(&self, p: Vec3, mask: u32) -> Option<&ClusterInfo> {
        self.bsps
            .iter()
            .filter(|b| mask & (1 << b.index) != 0)
            .flat_map(|b| b.clusters.iter())
            .filter_map(|c| {
                let (lo, hi) = c.bounds?;
                (p.cmpge(lo).all() && p.cmple(hi).all()).then(|| ((hi - lo).element_product(), c))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, c)| c)
    }

    /// Every BSP the map has, as a mask.
    pub fn all_bsps(&self) -> u32 {
        self.bsps.iter().fold(0, |m, b| m | (1 << b.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(name: &str) -> Option<CacheFile> {
        let path = std::path::Path::new(
            r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps",
        )
        .join(format!("{name}.map"));
        path.exists().then(|| CacheFile::open(path).unwrap())
    }

    /// The numbers in `docs/format/zone-sets.md` section 5.
    #[test]
    fn halo_zone_sets_and_starts() {
        let Some(cache) = map("120_halo") else { return };
        let mut scene = Scene::default();
        decode(&cache, &mut scene).unwrap();
        let sets: Vec<(&str, u32)> = scene
            .zone_sets
            .iter()
            .map(|z| (z.name.as_str(), z.bsps))
            .collect();
        assert_eq!(
            sets,
            [
                ("120_00_01_02", 0x7),
                ("120_01_02_03", 0xE),
                ("120_03_05", 0x28),
                ("120_07_06_04_03", 0x458),
                ("120_04_100_110", 0x190),
                ("120_100_110", 0x180),
                ("120_110_120", 0x300),
                ("120_120_130", 0x201),
            ]
        );
        assert_eq!(scene.starts.len(), 4);
        let s = &scene.starts[0];
        assert!((s.position - Vec3::new(-263.64, -433.77, -5.052)).length() < 0.01);
        assert_eq!(scene.trigger_volumes.len(), 254);
        assert_eq!(scene.trigger_volumes[1].name, "begin_zone_set:120_01_02_03");
        // The kill box under the start of the Warthog run, corner
        // (-96.3, 37.2, -158) extending (709, 472, 88.8).
        let kills: Vec<&TriggerVolume> = scene.trigger_volumes.iter().filter(|v| v.kills()).collect();
        assert!(kills.len() > 20, "{} kill volumes", kills.len());
        assert!(kills.iter().any(|v| v.contains(Vec3::new(300.0, 200.0, -100.0))));
        assert!(!kills.iter().any(|v| v.contains(scene.starts[0].position)));
        assert_eq!(scene.zone_switches.len(), 17);
        let z = &scene.zone_switches[2];
        assert_eq!((z.flags, z.begin, z.volume, z.commit), (1, Some(4), 3, None));
        assert_eq!(
            scene.trigger_volumes[z.volume as usize].name,
            "begin_zone_set:120_04_100_110:*"
        );
    }

    #[test]
    fn jungle_start_faces_its_yaw() {
        let Some(cache) = map("010_jungle") else { return };
        let mut scene = Scene::default();
        decode(&cache, &mut scene).unwrap();
        assert_eq!(scene.zone_sets.len(), 13);
        assert!((scene.starts[0].yaw - -2.2655).abs() < 1e-3);
    }
}
