//! Everything a set of effects needs, read once: the effects themselves
//! and every effect, particle, light and contrail they reach, and the
//! bitmaps those draw with.

use std::collections::{BTreeSet, HashMap};

use blam_cache::CacheFile;
use blam_texture::bitmap::Sequence;

use crate::tags::*;

/// The effects reachable from some root effects.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub effects: HashMap<u32, Effect>,
    pub particles: HashMap<u32, Particle>,
    pub lights: HashMap<u32, Light>,
    pub contrails: HashMap<u32, ContrailSystem>,
    /// Every bitmap a particle or contrail samples.
    pub bitmaps: BTreeSet<u32>,
    /// Sprite sequences of those bitmaps (only the ones that have any).
    pub sequences: HashMap<u32, Vec<Sequence>>,
    /// Effect names to tags.
    pub by_name: HashMap<String, u32>,
    /// The sounds the effects play, by tag, with their paths.
    pub sound_names: HashMap<u32, String>,
    /// What could not be read, for the log.
    pub errors: Vec<String>,
}

impl Library {
    /// Reads `roots` (effect tag indices) and everything they reach.
    pub fn load(cache: &CacheFile, roots: &[u32]) -> Library {
        let mut lib = Library::default();
        for &r in roots {
            lib.add_effect(cache, r, 0);
        }
        for &b in &lib.bitmaps {
            match blam_texture::bitmap::sequences(cache, b) {
                Ok(s) if !s.is_empty() => {
                    lib.sequences.insert(b, s);
                }
                Ok(_) => {}
                Err(e) => lib.errors.push(format!("bitmap {b}: {e}")),
            }
        }
        lib
    }

    /// Reads one more effect (and what it reaches) into the library.
    pub fn add(&mut self, cache: &CacheFile, effect: u32) {
        self.add_effect(cache, effect, 0);
    }

    pub fn effect(&self, tag: u32) -> Option<&Effect> {
        self.effects.get(&tag)
    }

    /// An effect by its tag path (case-insensitive).
    pub fn effect_named(&self, name: &str) -> Option<&Effect> {
        self.by_name.get(&name.to_ascii_lowercase()).and_then(|t| self.effects.get(t))
    }

    fn add_effect(&mut self, cache: &CacheFile, tag: u32, depth: u32) {
        if self.effects.contains_key(&tag) || depth > 6 {
            return;
        }
        let effect = match read_effect(cache, tag) {
            Ok(e) => e,
            Err(e) => {
                self.errors.push(format!("effect {tag}: {e}"));
                return;
            }
        };
        self.by_name.insert(effect.name.to_ascii_lowercase(), tag);
        let mut reached = Vec::new();
        let mut particles = Vec::new();
        for event in &effect.events {
            for part in &event.parts {
                if let Some(r) = part.what {
                    reached.push(r);
                }
            }
            particles.extend(event.systems.iter().filter_map(|s| s.particle));
        }
        self.effects.insert(tag, effect);
        for p in particles {
            self.add_particle(cache, p, depth);
        }
        for r in reached {
            self.add_ref(cache, r, depth);
        }
    }

    fn add_ref(&mut self, cache: &CacheFile, r: Ref, depth: u32) {
        match r.class.as_str() {
            "snd!" => {
                let name = cache.tags.get(r.tag).map_or_else(String::new, |t| t.name().to_string());
                self.sound_names.insert(r.tag, name);
            }
            "effe" => self.add_effect(cache, r.tag, depth + 1),
            "ligh" if !self.lights.contains_key(&r.tag) => match read_light(cache, r.tag) {
                Ok(l) => {
                    self.lights.insert(r.tag, l);
                }
                Err(e) => self.errors.push(format!("light {}: {e}", r.tag)),
            },
            "cntl" if !self.contrails.contains_key(&r.tag) => match read_contrail(cache, r.tag) {
                Ok(c) => {
                    for c in &c.contrails {
                        self.bitmaps.extend(shader_bitmaps(&c.shader));
                    }
                    self.contrails.insert(r.tag, c);
                }
                Err(e) => self.errors.push(format!("contrail {}: {e}", r.tag)),
            },
            "prt3" => self.add_particle(cache, r.tag, depth + 1),
            _ => {}
        }
    }

    fn add_particle(&mut self, cache: &CacheFile, tag: u32, depth: u32) {
        if self.particles.contains_key(&tag) {
            return;
        }
        let particle = match read_particle(cache, tag) {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(format!("particle {tag}: {e}"));
                return;
            }
        };
        self.bitmaps.extend(shader_bitmaps(&particle.shader));
        let attached: Vec<Ref> = particle.attachments.iter().map(|a| a.what).collect();
        self.particles.insert(tag, particle);
        for r in attached {
            self.add_ref(cache, r, depth + 1);
        }
    }
}

/// The bitmaps a particle or contrail shader samples: its colour, alpha
/// and palette maps.
pub fn shader_bitmaps(s: &blam_texture::Shader) -> Vec<u32> {
    ["base_map", "alpha_map", "palette"]
        .iter()
        .filter_map(|u| s.map(u).and_then(|m| m.bitmap))
        .collect()
}
