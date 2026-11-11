//! The sound tags of a map: `snd!` through the `ugh!` sound gestalt,
//! looping sounds (`lsnd`), sound scenery (`ssce`) placed by the scenario,
//! and each BSP's acoustics palette (background sound per cluster).
//! `docs/format/sound.md` sections 4 to 7. Offsets are from MCC Halo 3 U13
//! and were found by walking the bytes; nothing here comes from other
//! projects' code. Reach's maps hold the same tags at other offsets
//! ([`Layout`], `sound.md` 12).

use blam_cache::{Block, CacheFile, Engine, StringId, TagEntry};

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

/// A looping sound (`lsnd`, `sound.md` 5).
#[derive(Clone, Debug, Default)]
pub struct Looping {
    pub tag: u32,
    pub name: String,
    /// Header flags (`+0x00`): [`Looping::NOT_A_LOOP`],
    /// [`Looping::STOPS_MUSIC`], and bits 3-6 seen on engines and
    /// ambience (not used here).
    pub flags: u32,
    /// Seconds a music piece plays before it ends by itself (`+0x04`;
    /// 0: until a script stops it). Only music sets it.
    pub music_time: f32,
    /// (full volume within, silent beyond), world units (`+0x0C`, `+0x10`):
    /// how far a placed copy carries.
    pub distance: (f32, f32),
    /// Sound class (`+0x24`): 32 on every music piece.
    pub class: i16,
    pub tracks: Vec<Track>,
    pub details: Vec<Detail>,
}

impl Looping {
    /// Header bit 1: plays its loop once instead of looping (stingers:
    /// `120_music_12`, `sound\music\stingers\orch3`).
    pub const NOT_A_LOOP: u32 = 1 << 1;
    /// Header bit 2: starting it ends whatever other music is playing
    /// (`120_music_15`, the Warthog run).
    pub const STOPS_MUSIC: u32 = 1 << 2;
    /// The sound class of music (every `lsnd` under a `\music\` folder).
    pub const MUSIC_CLASS: i16 = 32;

    pub fn not_a_loop(&self) -> bool {
        self.flags & Self::NOT_A_LOOP != 0
    }
    pub fn stops_music(&self) -> bool {
        self.flags & Self::STOPS_MUSIC != 0
    }
    pub fn is_music(&self) -> bool {
        self.class == Self::MUSIC_CLASS || self.name.to_ascii_lowercase().contains("\\music\\")
    }
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
    /// Seconds to cross-fade between the loop and the alternate loop when
    /// [`Track::CROSSFADE_ALT`] is set (`+0x88`; 10 on `120_music_13`).
    pub alt_fade: f32,
    /// A second time at `+0x8C` (0 on every music track; equal to `+0x88`
    /// on vehicle skid loops). Not used.
    pub alt_fade_out: f32,
}

impl Track {
    /// Bit 0: fade in over `fade_in` when started.
    pub const FADE_IN: u32 = 1 << 0;
    /// Bit 1: fade out over `fade_out` when stopped (music without an outro).
    pub const FADE_OUT: u32 = 1 << 1;
    /// Bit 2: switch to and from the alternate loop by cross-fading over
    /// `alt_fade` at once, instead of at the end of the playing loop.
    pub const CROSSFADE_ALT: u32 = 1 << 2;

    pub fn fades_in(&self) -> bool {
        self.flags & Self::FADE_IN != 0 && self.fade_in > 0.0
    }
    pub fn fades_out(&self) -> bool {
        self.flags & Self::FADE_OUT != 0 && self.fade_out > 0.0
    }
    pub fn crossfades_alt(&self) -> bool {
        self.flags & Self::CROSSFADE_ALT != 0 && self.alt_fade > 0.0
    }
    /// Every sound the track can play, in slot order (in, loop, out, alt
    /// loop, alt out, alt transition in, alt transition out).
    pub fn sounds(&self) -> [Option<u32>; 7] {
        [
            self.intro,
            self.looped,
            self.outro,
            self.alt_loop,
            self.alt_out,
            self.alt_trans_in,
            self.alt_trans_out,
        ]
    }
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
    /// The `snd!` and `lsnd` tags the scenario's scripts name (its script
    /// tag references, `scnr + 0x404`; `halo-script.md` 1), by tag index.
    pub script_sounds: Vec<u32>,
}

/// Where a game keeps what this module reads: Halo 3's offsets
/// (`sound.md` 4 to 7) and Reach's (`sound.md` 12).
#[derive(Clone, Copy, Debug)]
struct Layout {
    reach: bool,
    /// `ugh!`: playback parameters, pitch ranges, permutations.
    playbacks: usize,
    playback_size: usize,
    pitch_ranges: usize,
    permutations: usize,
    permutation_size: usize,
    /// Permutation: the FMOD id.
    permutation_id: usize,
    /// `snd!`: playback parameter index, longest permutation.
    sound_playback: usize,
    sound_max_play: usize,
    /// `lsnd`: sound class, tracks, details.
    loop_class: usize,
    tracks: usize,
    track_size: usize,
    /// Track: fade in and out times, the seven sound slots (in, loop,
    /// out, alt loop, alt out, alt transition in and out), the alternate
    /// cross-fade times (`None`: not found in this game).
    track_fades: [usize; 2],
    track_slots: [usize; 7],
    track_alt_fades: Option<[usize; 2]>,
    details: usize,
    detail_size: usize,
    /// Detail: period, gain, flags, yaw, pitch, distance.
    detail_fields: [usize; 6],
    /// Objects: the attachments block.
    attachments: usize,
    attachment_size: usize,
    /// `scnr`: sound scenery placements and palette, structure BSPs.
    scenery: usize,
    scenery_size: usize,
    scenery_palette: usize,
    /// Placement: origin BSP, volume type, height, distance bounds.
    scenery_bsp: usize,
    scenery_volume: usize,
    scenery_height: usize,
    scenery_distance: [usize; 2],
    bsps: usize,
    bsp_size: usize,
    /// `sbsp`: the acoustics palette; entry: background and inside loops,
    /// fade time (`None`: not found in this game).
    acoustics: usize,
    acoustics_size: usize,
    acoustics_loops: [usize; 2],
    acoustics_fade: Option<usize>,
    /// `scnr`: the scripts' tag references (`None`: not read).
    script_references: Option<usize>,
    /// `sncl` class: size, the four distances, gain bounds, dialogue
    /// ducking (dB, fade down, fade up).
    class_size: usize,
    class_distances: [usize; 4],
    class_gain: usize,
    class_duck: [usize; 3],
}

impl Layout {
    const HALO3: Layout = Layout {
        reach: false,
        playbacks: 0x10,
        playback_size: 68,
        pitch_ranges: 0x40,
        permutations: 0x4C,
        permutation_size: 20,
        permutation_id: 0x10,
        sound_playback: 0x0C,
        sound_max_play: 0x14,
        loop_class: 0x24,
        tracks: 0x28,
        track_size: 144,
        track_fades: [0x0C, 0x10],
        track_slots: [0x14, 0x24, 0x34, 0x44, 0x54, 0x68, 0x78],
        track_alt_fades: Some([0x88, 0x8C]),
        details: 0x34,
        detail_size: 60,
        detail_fields: [0x14, 0x1C, 0x20, 0x24, 0x2C, 0x34],
        attachments: 0xBC,
        attachment_size: ATTACHMENT_SIZE,
        scenery: 0x180,
        scenery_size: 112,
        scenery_palette: 0x18C,
        scenery_bsp: 0x3C,
        scenery_volume: 0x50,
        scenery_height: 0x54,
        scenery_distance: [0x5C, 0x60],
        bsps: 0x18,
        bsp_size: 108,
        acoustics: 0x118,
        acoustics_size: 84,
        acoustics_loops: [0x1C, 0x2C],
        acoustics_fade: Some(0x50),
        script_references: Some(SCRIPT_REFERENCES),
        class_size: 0x98,
        class_distances: [0x34, 0x38, 0x3C, 0x40],
        class_gain: 0x44,
        class_duck: [0x5C, 0x60, 0x68],
    };

    /// MCC Reach (`sound.md` 12).
    const REACH: Layout = Layout {
        reach: true,
        playbacks: 0x10,
        playback_size: 84,
        pitch_ranges: 0x4C,
        permutations: 0x58,
        permutation_size: 24,
        permutation_id: 0x14,
        sound_playback: 0x0E,
        sound_max_play: 0x18,
        loop_class: 0x18,
        tracks: 0x20,
        track_size: 0xB0,
        track_fades: [0x7C, 0x84],
        track_slots: [0x0C, 0x1C, 0x2C, 0x3C, 0x4C, 0x5C, 0x6C],
        track_alt_fades: None,
        details: 0x2C,
        detail_size: 0x48,
        detail_fields: [0x18, 0x20, 0x2C, 0x30, 0x38, 0x40],
        attachments: 0x104,
        attachment_size: ATTACHMENT_SIZE,
        scenery: 0x1C8,
        scenery_size: 0x8C,
        scenery_palette: 0x1D4,
        scenery_bsp: 0x40,
        scenery_volume: 0x54,
        scenery_height: 0x58,
        scenery_distance: [0x78, 0x84],
        bsps: 0x50,
        bsp_size: 0xAC,
        acoustics: 0x18C,
        acoustics_size: 0x78,
        acoustics_loops: [0x20, 0x30],
        acoustics_fade: None,
        script_references: None,
        class_size: 0xD4,
        class_distances: [0x38, 0x3C, 0x48, 0x54],
        class_gain: 0x5C,
        class_duck: [0x80, 0x84, 0x8C],
    };

    fn of(cache: &CacheFile) -> Layout {
        match cache.engine() {
            Engine::Halo3 => Layout::HALO3,
            Engine::Reach => Layout::REACH,
        }
    }
}

/// Reads sound tags. Holds the gestalt's block headers.
pub struct SoundTags<'a> {
    cache: &'a CacheFile,
    layout: Layout,
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
        let layout = Layout::of(cache);
        Ok(SoundTags {
            cache,
            layout,
            playbacks: blk(layout.playbacks)?,
            names: blk(0x28)?,
            pitch_ranges: blk(layout.pitch_ranges)?,
            permutations: blk(layout.permutations)?,
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

    /// Whether the map is Reach's: its weapons, scripts and mission
    /// dialogue are not read (`sound.md` 12).
    pub fn is_reach(&self) -> bool {
        self.layout.reach
    }

    /// A `snd!` tag (36 bytes of tag data, `sound.md` 4.1).
    pub fn sound(&self, index: u32) -> Result<Sound> {
        let (t, o) = self.tag(index, "snd!")?;
        let c = self.cache;
        let y = &self.layout;
        let mut r = c.reader_at(o);
        let flags = r.u16()?;
        let class = r.u8()?;
        let _rate = r.u8()?;
        let encoding = r.u8()?;
        let pitch_range = c.reader_at(o + 0x06).i16()?;
        let playback_index = c.reader_at(o + y.sound_playback).i16()?;
        let max_play_ms = c.reader_at(o + y.sound_max_play).i32()?;
        let mut playback = Playback::default();
        if let Some(p) = u32::try_from(playback_index)
            .ok()
            .and_then(|i| self.playbacks.element(i, y.playback_size))
        {
            if y.reach {
                // Reach's distances (`sound.md` 12.2): the don't-play and
                // attack distances, the minimum and the maximum. No flags
                // say which are unused: zero means the class's.
                let f = |at: usize| c.reader_at(p + at).f32();
                playback.distances = [f(0x08)?, f(0x0C)?, f(0x18)?, f(0x24)?];
                playback.flags = (0..4).filter(|n| playback.distances[*n] <= 0.0).fold(0, |m, n| m | 1 << n);
                playback.gain_db = f(0x2C)?;
            } else {
                let mut r = c.reader_at(p);
                playback.flags = r.u32()?;
                playback.distances = r.f32x4()?;
                playback.gain_db = c.reader_at(p + 0x1C).f32()?;
            }
        }
        let mut permutations = Vec::new();
        if let Some(pr) = u32::try_from(pitch_range)
            .ok()
            .and_then(|i| self.pitch_ranges.element(i, 12))
        {
            let first = c.reader_at(pr + 8).u16()? as u32;
            let count = ((c.reader_at(pr + 10).u16()? >> 4) & 0x3F) as u32;
            for p in first..first + count {
                let Some(q) = self.permutations.element(p, y.permutation_size) else {
                    break;
                };
                permutations.push(Permutation {
                    name: self.import_name(c.reader_at(q).u16()? as u32)?,
                    fmod_id: c.reader_at(q + y.permutation_id).u32()?,
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
        let y = &self.layout;
        let mut tracks = Vec::new();
        for e in c.reader_at(o + y.tracks).block()?.elements(y.track_size) {
            let mut r = c.reader_at(e + 4);
            let f = |at: usize| c.reader_at(e + at).f32();
            let [alt_fade, alt_fade_out] = match y.track_alt_fades {
                Some([a, b]) => [f(a)?, f(b)?],
                None => [0.0; 2],
            };
            let s = y.track_slots;
            tracks.push(Track {
                name: sid(e)?,
                flags: r.u32()?,
                gain_db: r.f32()?,
                fade_in: f(y.track_fades[0])?,
                fade_out: f(y.track_fades[1])?,
                intro: snd(e + s[0])?,
                looped: snd(e + s[1])?,
                outro: snd(e + s[2])?,
                alt_loop: snd(e + s[3])?,
                alt_out: snd(e + s[4])?,
                alt_trans_in: snd(e + s[5])?,
                alt_trans_out: snd(e + s[6])?,
                alt_fade,
                alt_fade_out,
            });
        }
        let mut details = Vec::new();
        for e in c.reader_at(o + y.details).block()?.elements(y.detail_size) {
            let f = |at: usize| c.reader_at(e + at).f32x2().map(|[a, b]| (a, b));
            let d = y.detail_fields;
            details.push(Detail {
                name: sid(e)?,
                sound: snd(e + 4)?,
                period: f(d[0])?,
                gain_db: c.reader_at(e + d[1]).f32()?,
                flags: c.reader_at(e + d[2]).u32()?,
                yaw: f(d[3])?,
                pitch: f(d[4])?,
                distance: f(d[5])?,
            });
        }
        Ok(Looping {
            tag: index,
            name: t.name().to_string(),
            flags: c.reader_at(o).u32()?,
            music_time: c.reader_at(o + 0x04).f32()?,
            distance: (c.reader_at(o + 0x0C).f32()?, c.reader_at(o + 0x10).f32()?),
            class: c.reader_at(o + y.loop_class).i16()?,
            tracks,
            details,
        })
    }

    /// The `lsnd` a sound scenery object carries: its first looping-sound
    /// attachment (`ssce + 0xBC`, the object attachments block).
    pub fn scenery_looping(&self, ssce: u32) -> Result<Option<u32>> {
        let (_, o) = self.tag(ssce, "ssce")?;
        let attachments = self.cache.reader_at(o + self.layout.attachments).block()?;
        for i in 0..attachments.count {
            let Some(e) = attachments.element(i, self.layout.attachment_size) else {
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
        let y = &self.layout;
        let mut out = MapSounds::default();

        let palette: Vec<Option<u32>> = c
            .reader_at(s + y.scenery_palette)
            .block()?
            .elements(16)
            .map(|e| tag_index(c, e, "ssce"))
            .collect::<Result<_>>()?;
        for e in c.reader_at(s + y.scenery).block()?.elements(y.scenery_size) {
            let pal = c.reader_at(e).i16()?;
            let Some(ssce) = usize::try_from(pal)
                .ok()
                .and_then(|p| palette.get(p).copied().flatten())
            else {
                continue;
            };
            let near = c.reader_at(e + y.scenery_distance[0]).f32()?;
            let far = c.reader_at(e + y.scenery_distance[1]).f32()?;
            out.scenery.push(SoundScenery {
                object: c
                    .tags
                    .get(ssce)
                    .map(|t| t.name().to_string())
                    .unwrap_or_default(),
                looping: self.scenery_looping(ssce)?,
                position: c.reader_at(e + 0x08).f32x3()?,
                origin_bsp: u32::try_from(c.reader_at(e + y.scenery_bsp).i16()?).ok(),
                volume_type: c.reader_at(e + y.scenery_volume).i32()?,
                height: c.reader_at(e + y.scenery_height).f32()?,
                distance: (far > 0.0).then_some((near, far)),
            });
        }

        for (i, e) in c.reader_at(s + y.bsps).block()?.elements(y.bsp_size).enumerate() {
            let Some(sbsp) = tag_index(c, e, "sbsp")? else {
                continue;
            };
            let b = c.tag_offset(c.tags.get(sbsp).unwrap())?;
            let mut palette = Vec::new();
            for a in c.reader_at(b + y.acoustics).block()?.elements(y.acoustics_size) {
                let id = c.reader_at(a).string_id()?;
                palette.push(Acoustics {
                    name: c.string(id).unwrap_or("").to_string(),
                    environment: tag_index(c, a + 0x04, "snde")?,
                    background: tag_index(c, a + y.acoustics_loops[0], "lsnd")?,
                    inside: tag_index(c, a + y.acoustics_loops[1], "lsnd")?,
                    fade: match y.acoustics_fade {
                        Some(at) => c.reader_at(a + at).f32()?,
                        None => 0.5,
                    },
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

        // The scripts' tag references: plain 16-byte references.
        let script_references = match y.script_references {
            Some(at) => Some(c.reader_at(s + at).block()?),
            None => None,
        };
        for e in script_references.iter().flat_map(|b| b.elements(16)) {
            let r = c.reader_at(e).tag_ref()?;
            if let Some(i) = r.index
                && c.tags.get(i).is_some_and(|t| matches!(c.tags.class_of(t).code.as_str(), "snd!" | "lsnd"))
                && !out.script_sounds.contains(&i)
            {
                out.script_sounds.push(i);
            }
        }
        Ok(out)
    }

    /// The map's mission dialogue (`mdlg`, one tag per campaign map, none
    /// on multiplayer maps): the lines the scripts play by name
    /// (`ai_play_line_on_object none 120mg_030`), `sound.md` 11.
    pub fn mission_dialogue(&self) -> Result<Vec<DialogueLine>> {
        let c = self.cache;
        let mut out = Vec::new();
        if self.layout.reach {
            return Ok(out);
        }
        for t in c.tags.of_class("mdlg") {
            let o = c.tag_offset(t)?;
            for l in c.reader_at(o).block()?.elements(DIALOGUE_LINE_SIZE) {
                let mut variants = Vec::new();
                for v in c.reader_at(l + 4).block()?.elements(DIALOGUE_VARIANT_SIZE) {
                    variants.push(DialogueVariant {
                        speaker: self.string_at(v)?,
                        sound: tag_index(c, v + 4, "snd!")?,
                        effect: self.string_at(v + 0x14)?,
                    });
                }
                out.push(DialogueLine {
                    name: self.string_at(l)?,
                    variants,
                    effect: self.string_at(l + 0x10)?,
                });
            }
        }
        Ok(out)
    }

    fn string_at(&self, at: usize) -> Result<String> {
        let id = self.cache.reader_at(at).string_id()?;
        Ok(self.cache.string(id).unwrap_or("").to_string())
    }
}

/// One line of a map's mission dialogue (`mdlg + 0x00`, 20 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogueLine {
    /// What the scripts name it by: `120mg_030`.
    pub name: String,
    /// Who may say it; nearly always one (120_halo: one line of 213 has
    /// two, the Master Chief's and Guilty Spark's takes of `120lb_220`).
    pub variants: Vec<DialogueVariant>,
    /// The line's sound effect by name, `""` for none: `cortana_mission`
    /// (Cortana in the Chief's head), `radio_default`, `radio_3d`,
    /// `object`. Not applied (`sound.md` 11).
    pub effect: String,
}

/// One speaker's take of a line (24 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogueVariant {
    /// The speaker's three-letter code: `cor` Cortana, `jon` Johnson, `arb`
    /// the Arbiter, `gsp` Guilty Spark, `mas` the Master Chief, `grv` the
    /// Gravemind, `sma` a marine.
    pub speaker: String,
    /// The `snd!` tag (its permutations are in the language bank).
    pub sound: Option<u32>,
    /// A per-variant effect (`radio_default`, `radio_3d`), `""` for none.
    pub effect: String,
}

impl DialogueLine {
    /// The sound of the first variant that has one.
    pub fn sound(&self) -> Option<(u32, &str)> {
        self.variants.iter().find_map(|v| v.sound.map(|s| (s, v.speaker.as_str())))
    }
}

const DIALOGUE_LINE_SIZE: usize = 20;
const DIALOGUE_VARIANT_SIZE: usize = 24;

/// The scenario's block of tag references its scripts use.
const SCRIPT_REFERENCES: usize = 0x404;

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
    /// The class's gain range, dB (`+0x44` lower, `+0x48` upper; inferred
    /// from the values, `sound.md` 9): 0 on music, -3 on dialogue, -12 to
    /// -15 on weapons, impacts, footsteps, vehicles and ambience. Most
    /// classes give one value twice.
    pub gain_bounds_db: [f32; 2],
    /// How far the class is turned down while scripted dialogue plays, dB
    /// (`+0x5C`; 0 = not at all), and the fade down / back up times, s
    /// (`+0x60`, `+0x68`). Inferred (`sound.md` 9 and 11): of the three
    /// ducking slots this is the one that turns music down 3 dB, effects 6
    /// and unit (AI) dialogue 6, and leaves ambience and the mission
    /// dialogue classes alone.
    pub dialogue_duck_db: f32,
    pub dialogue_duck_fade: [f32; 2],
}

impl SoundClass {
    /// Music's class (`lsnd + 0x24` on every music piece).
    pub const MUSIC: u8 = 32;

    /// The level the class plays its sounds at, dB: the upper end of its
    /// gain range.
    pub fn gain_db(&self) -> f32 {
        self.gain_bounds_db[1]
    }
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

impl SoundTags<'_> {
    /// The map's sound classes, by index (empty without an `sncl` tag).
    pub fn classes(&self) -> Result<Vec<SoundClass>> {
        let c = self.cache;
        let Some(t) = c.tags.of_class("sncl").next() else {
            return Ok(Vec::new());
        };
        let o = c.tag_offset(t)?;
        let y = &self.layout;
        let mut out = Vec::new();
        for e in c.reader_at(o).block()?.elements(y.class_size) {
            let mut r = c.reader_at(e);
            let max_per_tag = r.i16()?;
            let max_per_object = r.i16()?;
            let f = |at: usize| c.reader_at(e + at).f32();
            let d = y.class_distances;
            out.push(SoundClass {
                max_per_tag,
                max_per_object,
                distances: [f(d[0])?, f(d[1])?, f(d[2])?, f(d[3])?],
                gain_bounds_db: [f(y.class_gain)?, f(y.class_gain + 4)?],
                dialogue_duck_db: f(y.class_duck[0])?,
                dialogue_duck_fade: [f(y.class_duck[1])?, f(y.class_duck[2])?],
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
