//! The sound tags of a map: `snd!` through the `ugh!` sound gestalt,
//! looping sounds (`lsnd`), sound scenery (`ssce`) placed by the scenario,
//! and each BSP's acoustics palette (background sound per cluster).
//! `docs/format/sound.md` sections 4 to 7. Offsets are from MCC Halo 3 U13
//! and were found by walking the bytes; nothing here comes from other
//! projects' code.

use blam_cache::{Block, CacheFile, StringId, TagEntry};

use crate::{Error, Result};

/// A `snd!` tag, with what the gestalt says about it.
#[derive(Clone, Debug)]
pub struct Sound {
    pub tag: u32,
    pub name: String,
    pub flags: u16,
    pub class: u8,
    /// 0 mono, 1 stereo (the permutations' channel layout).
    pub encoding: u8,
    pub playback: Playback,
    /// The first pitch range's permutations; the others (pitch-dependent
    /// variants, rare outside vehicles) are not read.
    pub permutations: Vec<Permutation>,
    /// The longest permutation, ms.
    pub max_play_ms: i32,
}

#[derive(Clone, Debug)]
pub struct Permutation {
    pub name: String,
    /// The FMOD bank id ([`crate::fsb::Info::id`]).
    pub fmod_id: u32,
}

/// The playback parameters a sound points at (`ugh! + 0x10`, 68 bytes).
#[derive(Clone, Copy, Debug, Default)]
pub struct Playback {
    /// Which distances below are switched off (bit n: distance n).
    pub flags: u32,
    /// Four distances, world units. Halo 3 attenuates between the last
    /// two: full volume within `distances[2]`, silent beyond
    /// `distances[3]` (`sound.md` 4.3; the first two are unverified).
    pub distances: [f32; 4],
    /// Gain in dB.
    pub gain_db: f32,
}

impl Playback {
    /// (full-volume distance, silent distance), world units.
    pub fn bounds(&self) -> (f32, f32) {
        let max = self.distances[3];
        let min = self.distances[2].min(max);
        (min, max)
    }
}

/// A looping sound (`lsnd`).
#[derive(Clone, Debug, Default)]
pub struct Looping {
    pub tag: u32,
    pub name: String,
    pub tracks: Vec<Track>,
    pub details: Vec<Detail>,
}

/// One layer of a looping sound: an optional intro, the loop and an
/// optional outro, and an alternate loop with its transitions (music).
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub name: String,
    pub flags: u32,
    pub gain_db: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub intro: Option<u32>,
    pub looped: Option<u32>,
    pub outro: Option<u32>,
    pub alt_loop: Option<u32>,
    pub alt_out: Option<u32>,
    pub alt_trans_in: Option<u32>,
    pub alt_trans_out: Option<u32>,
}

/// A one-shot a looping sound scatters around the listener now and then
/// (birds, gusts).
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub name: String,
    pub sound: Option<u32>,
    /// Seconds between plays.
    pub period: (f32, f32),
    pub gain_db: f32,
    pub flags: u32,
    /// Radians.
    pub yaw: (f32, f32),
    pub pitch: (f32, f32),
    /// World units from the listener.
    pub distance: (f32, f32),
}

/// A sound scenery object placed in the scenario.
#[derive(Clone, Debug)]
pub struct SoundScenery {
    pub object: String,
    /// The looping sound the object carries (its first `lsnd` attachment).
    pub looping: Option<u32>,
    /// World units, Halo frame (Z up).
    pub position: [f32; 3],
    /// The BSP the object belongs to, `None`: always present.
    pub origin_bsp: Option<u32>,
    /// 0 sphere, 1 vertical cylinder (by the placement editor's enum).
    pub volume_type: i32,
    pub height: f32,
    /// The placement's own distance bounds, when it sets them.
    pub distance: Option<(f32, f32)>,
}

/// An entry of a BSP's acoustics palette (`sbsp + 0x118`, 84 bytes):
/// what a cluster sounds like.
#[derive(Clone, Debug, Default)]
pub struct Acoustics {
    pub name: String,
    /// `snde` sound environment (reverb), tag index.
    pub environment: Option<u32>,
    /// The background `lsnd` heard anywhere in the cluster.
    pub background: Option<u32>,
    /// A second `lsnd` "inside cluster" sound, rarely set.
    pub inside: Option<u32>,
    /// Seconds to fade between backgrounds (`+0x50`, unverified meaning).
    pub fade: f32,
}

/// A BSP's acoustics palette, by scenario BSP index.
#[derive(Clone, Debug, Default)]
pub struct BspAcoustics {
    pub bsp: u32,
    pub palette: Vec<Acoustics>,
}

/// The map's sounds the viewer plays.
#[derive(Clone, Debug, Default)]
pub struct MapSounds {
    pub scenery: Vec<SoundScenery>,
    pub bsps: Vec<BspAcoustics>,
    /// Looping sounds under a `music` folder, by tag index (scripts start
    /// them; the viewer plays them by hand).
    pub music: Vec<(u32, String)>,
}

/// Reads sound tags. Holds the gestalt's block headers.
pub struct SoundTags<'a> {
    cache: &'a CacheFile,
    playbacks: Block,
    names: Block,
    pitch_ranges: Block,
    permutations: Block,
}

fn tag_index(cache: &CacheFile, at: usize, class: &str) -> Result<Option<u32>> {
    let r = cache.reader_at(at).tag_ref()?;
    Ok(r.index.filter(|i| {
        cache
            .tags
            .get(*i)
            .is_some_and(|t| cache.tags.class_of(t).code == class)
    }))
}

impl<'a> SoundTags<'a> {
    pub fn new(cache: &'a CacheFile) -> Result<SoundTags<'a>> {
        let ugh = cache
            .global_tag("ugh!")
            .ok_or_else(|| Error::Invalid("no sound gestalt (ugh!)".into()))?;
        let base = cache.tag_offset(ugh)?;
        let blk = |off: usize| cache.reader_at(base + off).block();
        Ok(SoundTags {
            cache,
            playbacks: blk(0x10)?,
            names: blk(0x28)?,
            pitch_ranges: blk(0x40)?,
            permutations: blk(0x4C)?,
        })
    }

    pub fn cache(&self) -> &'a CacheFile {
        self.cache
    }

    fn import_name(&self, i: u32) -> Result<String> {
        let Some(o) = self.names.element(i, 4) else {
            return Ok(String::new());
        };
        let id = self.cache.reader_at(o).string_id()?;
        Ok(self.cache.string(id).unwrap_or("").to_string())
    }

    fn tag(&self, index: u32, class: &str) -> Result<(&'a TagEntry, usize)> {
        let t = self
            .cache
            .tags
            .get(index)
            .filter(|t| self.cache.tags.class_of(t).code == class)
            .ok_or_else(|| Error::Invalid(format!("tag {index} is not a {class}")))?;
        Ok((t, self.cache.tag_offset(t)?))
    }

    /// A `snd!` tag (36 bytes of tag data, `sound.md` 4.1).
    pub fn sound(&self, index: u32) -> Result<Sound> {
        let (t, o) = self.tag(index, "snd!")?;
        let c = self.cache;
        let mut r = c.reader_at(o);
        let flags = r.u16()?;
        let class = r.u8()?;
        let _rate = r.u8()?;
        let encoding = r.u8()?;
        let pitch_range = c.reader_at(o + 0x06).i16()?;
        let playback_index = c.reader_at(o + 0x0C).i16()?;
        let max_play_ms = c.reader_at(o + 0x14).i32()?;
        let mut playback = Playback::default();
        if let Some(p) = u32::try_from(playback_index)
            .ok()
            .and_then(|i| self.playbacks.element(i, 68))
        {
            let mut r = c.reader_at(p);
            playback.flags = r.u32()?;
            playback.distances = r.f32x4()?;
            playback.gain_db = c.reader_at(p + 0x1C).f32()?;
        }
        let mut permutations = Vec::new();
        if let Some(pr) = u32::try_from(pitch_range)
            .ok()
            .and_then(|i| self.pitch_ranges.element(i, 12))
        {
            let first = c.reader_at(pr + 8).u16()? as u32;
            let count = ((c.reader_at(pr + 10).u16()? >> 4) & 0x3F) as u32;
            for p in first..first + count {
                let Some(q) = self.permutations.element(p, 20) else {
                    break;
                };
                permutations.push(Permutation {
                    name: self.import_name(c.reader_at(q).u16()? as u32)?,
                    fmod_id: c.reader_at(q + 16).u32()?,
                });
            }
        }
        Ok(Sound {
            tag: index,
            name: t.name().to_string(),
            flags,
            class,
            encoding,
            playback,
            permutations,
            max_play_ms,
        })
    }

    /// A looping sound (`sound.md` 5).
    pub fn looping(&self, index: u32) -> Result<Looping> {
        let (t, o) = self.tag(index, "lsnd")?;
        let c = self.cache;
        let sid = |at: usize| -> Result<String> {
            let id: StringId = c.reader_at(at).string_id()?;
            Ok(c.string(id).unwrap_or("").to_string())
        };
        let snd = |at: usize| tag_index(c, at, "snd!");
        let mut tracks = Vec::new();
        for e in c.reader_at(o + 0x28).block()?.elements(144) {
            let mut r = c.reader_at(e + 4);
            tracks.push(Track {
                name: sid(e)?,
                flags: r.u32()?,
                gain_db: r.f32()?,
                fade_in: r.f32()?,
                fade_out: r.f32()?,
                intro: snd(e + 0x14)?,
                looped: snd(e + 0x24)?,
                outro: snd(e + 0x34)?,
                alt_loop: snd(e + 0x44)?,
                alt_out: snd(e + 0x54)?,
                alt_trans_in: snd(e + 0x68)?,
                alt_trans_out: snd(e + 0x78)?,
            });
        }
        let mut details = Vec::new();
        for e in c.reader_at(o + 0x34).block()?.elements(60) {
            let f = |at: usize| c.reader_at(e + at).f32x2().map(|[a, b]| (a, b));
            details.push(Detail {
                name: sid(e)?,
                sound: snd(e + 4)?,
                period: f(0x14)?,
                gain_db: c.reader_at(e + 0x1C).f32()?,
                flags: c.reader_at(e + 0x20).u32()?,
                yaw: f(0x24)?,
                pitch: f(0x2C)?,
                distance: f(0x34)?,
            });
        }
        Ok(Looping {
            tag: index,
            name: t.name().to_string(),
            tracks,
            details,
        })
    }

    /// The `lsnd` a sound scenery object carries: its first looping-sound
    /// attachment (`ssce + 0xBC`, the object attachments block).
    pub fn scenery_looping(&self, ssce: u32) -> Result<Option<u32>> {
        let (_, o) = self.tag(ssce, "ssce")?;
        let attachments = self.cache.reader_at(o + 0xBC).block()?;
        for i in 0..attachments.count {
            let Some(e) = attachments.element(i, ATTACHMENT_SIZE) else {
                break;
            };
            if let Some(l) = tag_index(self.cache, e, "lsnd")? {
                return Ok(Some(l));
            }
        }
        Ok(None)
    }

    /// Everything the viewer plays from this map.
    pub fn map_sounds(&self) -> Result<MapSounds> {
        let c = self.cache;
        let scnr = c
            .scenario()
            .ok_or_else(|| Error::Invalid("no scenario".into()))?;
        let s = c.tag_offset(scnr)?;
        let mut out = MapSounds::default();

        let palette: Vec<Option<u32>> = c
            .reader_at(s + 0x18C)
            .block()?
            .elements(16)
            .map(|e| tag_index(c, e, "ssce"))
            .collect::<Result<_>>()?;
        for e in c.reader_at(s + 0x180).block()?.elements(112) {
            let pal = c.reader_at(e).i16()?;
            let Some(ssce) = usize::try_from(pal)
                .ok()
                .and_then(|p| palette.get(p).copied().flatten())
            else {
                continue;
            };
            let near = c.reader_at(e + 0x5C).f32()?;
            let far = c.reader_at(e + 0x60).f32()?;
            out.scenery.push(SoundScenery {
                object: c
                    .tags
                    .get(ssce)
                    .map(|t| t.name().to_string())
                    .unwrap_or_default(),
                looping: self.scenery_looping(ssce)?,
                position: c.reader_at(e + 0x08).f32x3()?,
                origin_bsp: u32::try_from(c.reader_at(e + 0x3C).i16()?).ok(),
                volume_type: c.reader_at(e + 0x50).i32()?,
                height: c.reader_at(e + 0x54).f32()?,
                distance: (far > 0.0).then_some((near, far)),
            });
        }

        for (i, e) in c.reader_at(s + 0x18).block()?.elements(108).enumerate() {
            let Some(sbsp) = tag_index(c, e, "sbsp")? else {
                continue;
            };
            let b = c.tag_offset(c.tags.get(sbsp).unwrap())?;
            let mut palette = Vec::new();
            for a in c.reader_at(b + 0x118).block()?.elements(84) {
                let id = c.reader_at(a).string_id()?;
                palette.push(Acoustics {
                    name: c.string(id).unwrap_or("").to_string(),
                    environment: tag_index(c, a + 0x04, "snde")?,
                    background: tag_index(c, a + 0x1C, "lsnd")?,
                    inside: tag_index(c, a + 0x2C, "lsnd")?,
                    fade: c.reader_at(a + 0x50).f32()?,
                });
            }
            out.bsps.push(BspAcoustics {
                bsp: i as u32,
                palette,
            });
        }

        for t in c.tags.of_class("lsnd") {
            let n = t.name().to_ascii_lowercase();
            if n.contains("\\music\\") && !n.contains("main_menu") {
                out.music.push((t.index, t.name().to_string()));
            }
        }
        out.music.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(out)
    }
}

/// Size of an object attachment element (`weapon-sounds.md` 6: tag
/// reference, marker, two function names).
pub(crate) const ATTACHMENT_SIZE: usize = 32;

/// A sound class (`sncl`, one tag per map; a `snd!`'s class byte indexes
/// its block): the defaults a sound falls back to and how many copies may
/// play (`weapon-sounds.md` 7).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SoundClass {
    /// Copies of one sound tag playing at once.
    pub max_per_tag: i16,
    /// Copies of one sound tag one object may play at once.
    pub max_per_object: i16,
    /// The four default distances, world units (see [`Playback::distances`]).
    pub distances: [f32; 4],
}

impl Playback {
    /// The four distances in effect: the sound's own where its flags use
    /// them, else its class's defaults. In order: silent within, fully in
    /// at, full volume within, silent beyond (`weapon-sounds.md` 7; the
    /// meaning of the first two is inferred from the far-away sounds).
    pub fn distances_with(&self, class: Option<&SoundClass>) -> [f32; 4] {
        let mut d = self.distances;
        for (n, v) in d.iter_mut().enumerate() {
            if self.flags & (1 << n) != 0 {
                *v = class.map_or(0.0, |c| c.distances[n]);
            }
        }
        d
    }
}

/// Gain 0..1 at `distance` for the four distances of
/// [`Playback::distances_with`]: silent inside the first, fading in up to
/// the second, full up to the third, then falling as the square of the
/// remaining fraction to silence at the fourth. All zero: always full.
pub fn distance_gain(distance: f32, [quiet, fade_in, full, far]: [f32; 4]) -> f32 {
    if far <= 0.0 {
        return 1.0;
    }
    if quiet > 0.0 && distance < quiet {
        return 0.0;
    }
    if fade_in > quiet && distance < fade_in {
        return (distance - quiet) / (fade_in - quiet);
    }
    if distance <= full {
        1.0
    } else if distance >= far || far <= full {
        0.0
    } else {
        let t = (far - distance) / (far - full);
        t * t
    }
}

/// Size of a sound class element.
const CLASS_SIZE: usize = 0x98;

impl SoundTags<'_> {
    /// The map's sound classes, by index (empty without an `sncl` tag).
    pub fn classes(&self) -> Result<Vec<SoundClass>> {
        let c = self.cache;
        let Some(t) = c.tags.of_class("sncl").next() else {
            return Ok(Vec::new());
        };
        let o = c.tag_offset(t)?;
        let mut out = Vec::new();
        for e in c.reader_at(o).block()?.elements(CLASS_SIZE) {
            let mut r = c.reader_at(e);
            let max_per_tag = r.i16()?;
            let max_per_object = r.i16()?;
            out.push(SoundClass {
                max_per_tag,
                max_per_object,
                distances: c.reader_at(e + 0x34).f32x4()?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::distance_gain;

    #[test]
    fn near_and_far_layers() {
        // Weapon fire (class 4) and its far-away layer (class 15).
        let near = [0.0, 0.0, 3.0, 55.0];
        let far = [3.0, 20.0, 35.0, 70.0];
        assert_eq!(distance_gain(0.0, near), 1.0);
        assert_eq!(distance_gain(0.0, far), 0.0);
        assert!((distance_gain(11.5, far) - 0.5).abs() < 1e-6);
        assert_eq!(distance_gain(30.0, far), 1.0);
        assert!(distance_gain(60.0, far) > 0.0);
        assert_eq!(distance_gain(60.0, near), 0.0);
        // No distances at all: always heard.
        assert_eq!(distance_gain(500.0, [0.0; 4]), 1.0);
    }
}
