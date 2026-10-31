//! The `jmad` tag (model_animation_graph): skeleton, animation list, the
//! mode -> weapon class -> weapon type -> action tree that names which
//! animation plays when, and the resource groups the animation data lives
//! in. Offsets: `docs/format/animation.md` sections 2 to 5.

use blam_cache::{Block, CacheFile, ResourceId, TagEntry};

use crate::{Error, Result};

const NODE_SIZE: usize = 32;
const ANIMATION_SIZE: usize = 136;
const MODE_SIZE: usize = 40;
const CLASS_SIZE: usize = 28;
const TYPE_SIZE: usize = 52;
const REF_SIZE: usize = 8;
const BLEND_SCREEN_SIZE: usize = 28;
const INHERITANCE_SIZE: usize = 48;
const GROUP_SIZE: usize = 12;

/// One node of the graph's skeleton. The graph carries no rest pose: that
/// comes from the render model, matched by name.
#[derive(Clone, Debug)]
pub struct GraphNode {
    pub name: String,
    pub next_sibling: Option<u16>,
    pub first_child: Option<u16>,
    pub parent: Option<u16>,
    pub model_flags: u8,
    pub joint_flags: u8,
}

/// How an animation's values combine with what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimKind {
    /// Full local transforms of the nodes it has; the pose under it is replaced.
    Base,
    /// Differences from a reference pose, added on top of the base pose
    /// (aiming, firing, breathing).
    Overlay,
    /// Full local transforms that replace only the nodes the animation has
    /// (a hand gripping, a head turning).
    Replacement,
    Other(u8),
}

impl AnimKind {
    fn from_byte(b: u8) -> AnimKind {
        match b {
            0 => AnimKind::Base,
            1 => AnimKind::Overlay,
            2 => AnimKind::Replacement,
            n => AnimKind::Other(n),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            AnimKind::Base => "base",
            AnimKind::Overlay => "overlay",
            AnimKind::Replacement => "replacement",
            AnimKind::Other(_) => "other",
        }
    }
}

/// What the per-frame root motion ("frame info") holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementKind {
    None,
    /// dx, dy per frame.
    DxDy,
    /// dx, dy, dyaw per frame.
    DxDyDyaw,
    /// dx, dy, dz, dyaw per frame.
    DxDyDzDyaw,
    Other(u8),
}

impl MovementKind {
    pub(crate) fn from_byte(b: u8) -> MovementKind {
        match b {
            0 => MovementKind::None,
            1 => MovementKind::DxDy,
            2 => MovementKind::DxDyDyaw,
            3 => MovementKind::DxDyDzDyaw,
            n => MovementKind::Other(n),
        }
    }
    /// Floats per frame.
    pub fn floats(self) -> usize {
        match self {
            MovementKind::DxDy => 2,
            MovementKind::DxDyDyaw => 3,
            MovementKind::DxDyDzDyaw => 4,
            _ => 0,
        }
    }
}

/// One entry of the animation list (136 bytes in the tag).
#[derive(Clone, Debug)]
pub struct Animation {
    pub name: String,
    /// Chance weight among the variants of one action.
    pub weight: f32,
    pub loop_frame: i16,
    pub playback_flags: u16,
    /// Index into [`Graph::blend_screens`] (aim overlays).
    pub blend_screen: Option<u8>,
    pub node_count: u8,
    pub frame_count: u16,
    pub kind: AnimKind,
    pub movement: MovementKind,
    pub production_flags: u16,
    pub internal_flags: u16,
    pub node_checksum: u32,
    /// Matches the resource group member's checksum.
    pub checksum: u32,
    pub previous_variant: Option<u16>,
    pub next_variant: Option<u16>,
    /// Where the data is: resource group and member within it.
    pub group: Option<u16>,
    pub member: u16,
}

/// A labelled pointer at an animation: a mode-graph action or overlay.
#[derive(Clone, Debug)]
pub struct AnimRef {
    pub label: String,
    /// `None` for this graph, else an index into [`Graph::inheritance`].
    pub graph: Option<u16>,
    pub animation: u16,
}

#[derive(Clone, Debug, Default)]
pub struct WeaponType {
    pub label: String,
    pub actions: Vec<AnimRef>,
    pub overlays: Vec<AnimRef>,
}

#[derive(Clone, Debug, Default)]
pub struct WeaponClass {
    pub label: String,
    pub types: Vec<WeaponType>,
}

#[derive(Clone, Debug, Default)]
pub struct Mode {
    pub label: String,
    pub classes: Vec<WeaponClass>,
}

/// The aim range an overlay's frame grid covers (28 bytes).
#[derive(Clone, Debug)]
pub struct BlendScreen {
    pub label: String,
    /// Half-ranges in radians: how far left, right, up and down the grid
    /// reaches.
    pub yaw_left: f32,
    pub yaw_right: f32,
    pub pitch_up: f32,
    pub pitch_down: f32,
    /// Grid steps on each side of the centre (both 1 on every graph seen:
    /// a 3 x 3 grid of 9 frames).
    pub yaw_steps: (i16, i16),
    pub pitch_steps: (i16, i16),
}

/// A graph this one borrows animations from (48 bytes).
#[derive(Clone, Debug)]
pub struct Inheritance {
    /// Tag index of the inherited `jmad`.
    pub graph: Option<u32>,
    pub root_z_offset: f32,
    pub flags: u32,
}

/// One resource holding the data of `member_count` animations (12 bytes).
#[derive(Clone, Copy, Debug)]
pub struct ResourceGroup {
    pub member_count: i32,
    pub resource: ResourceId,
}

#[derive(Clone, Debug)]
pub struct Graph {
    /// Tag index of the `jmad`.
    pub tag: u32,
    pub name: String,
    /// The parent graph (tag index), if any.
    pub parent: Option<u32>,
    pub nodes: Vec<GraphNode>,
    pub animations: Vec<Animation>,
    pub modes: Vec<Mode>,
    pub blend_screens: Vec<BlendScreen>,
    pub inheritance: Vec<Inheritance>,
    pub groups: Vec<ResourceGroup>,
}

fn opt_u16(v: i16) -> Option<u16> {
    (v >= 0).then_some(v as u16)
}

fn sid(cache: &CacheFile, offset: usize) -> Result<String> {
    let id = cache.reader_at(offset).string_id()?;
    Ok(cache.string(id).unwrap_or("").to_string())
}

fn refs(cache: &CacheFile, block: Block) -> Result<Vec<AnimRef>> {
    block
        .elements(REF_SIZE)
        .map(|o| {
            let mut r = cache.reader_at(o + 4);
            let graph = opt_u16(r.i16()?);
            let animation = r.u16()?;
            Ok(AnimRef {
                label: sid(cache, o)?,
                graph,
                animation,
            })
        })
        .collect()
}

impl Graph {
    /// Reads a `jmad` tag.
    pub fn read(cache: &CacheFile, tag: &TagEntry) -> Result<Graph> {
        let class = &cache.tags.class_of(tag).code;
        if class.as_str() != "jmad" {
            return Err(Error::Invalid(format!("{} is a {class}, not a jmad", tag.name())));
        }
        let base = cache.tag_offset(tag)?;
        let parent = cache.reader_at(base).tag_ref()?.index;
        let block = |o: usize| cache.reader_at(base + o).block();

        let nodes = block(0x14)?
            .elements(NODE_SIZE)
            .map(|o| {
                let mut r = cache.reader_at(o + 4);
                Ok(GraphNode {
                    name: sid(cache, o)?,
                    next_sibling: opt_u16(r.i16()?),
                    first_child: opt_u16(r.i16()?),
                    parent: opt_u16(r.i16()?),
                    model_flags: r.u8()?,
                    joint_flags: r.u8()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let blend_screens = block(0x38)?
            .elements(BLEND_SCREEN_SIZE)
            .map(|o| {
                let mut r = cache.reader_at(o + 4);
                Ok(BlendScreen {
                    label: sid(cache, o)?,
                    yaw_left: r.f32()?,
                    yaw_right: r.f32()?,
                    yaw_steps: (r.i16()?, r.i16()?),
                    pitch_up: r.f32()?,
                    pitch_down: r.f32()?,
                    pitch_steps: (r.i16()?, r.i16()?),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let animations = block(0x50)?
            .elements(ANIMATION_SIZE)
            .map(|o| {
                let mut r = cache.reader_at(o + 4);
                let weight = r.f32()?;
                let loop_frame = r.i16()?;
                let playback_flags = r.u16()?;
                let blend_screen = r.i8()?;
                let _desired_compression = r.u8()?;
                let _current_compression = r.u8()?;
                let node_count = r.u8()?;
                let frame_count = r.u16()?;
                let kind = AnimKind::from_byte(r.u8()?);
                let movement = MovementKind::from_byte(r.u8()?);
                let production_flags = r.u16()?;
                let internal_flags = r.u16()?;
                let node_checksum = r.u32()?;
                let checksum = r.u32()?;
                r.skip(4);
                let previous_variant = opt_u16(r.i16()?);
                let next_variant = opt_u16(r.i16()?);
                let group = opt_u16(r.i16()?);
                let member = r.u16()?;
                Ok(Animation {
                    name: sid(cache, o)?,
                    weight,
                    loop_frame,
                    playback_flags,
                    blend_screen: u8::try_from(blend_screen).ok(),
                    node_count,
                    frame_count,
                    kind,
                    movement,
                    production_flags,
                    internal_flags,
                    node_checksum,
                    checksum,
                    previous_variant,
                    next_variant,
                    group,
                    member,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let modes = block(0x5C)?
            .elements(MODE_SIZE)
            .map(|mo| {
                let classes = cache
                    .reader_at(mo + 4)
                    .block()?
                    .elements(CLASS_SIZE)
                    .map(|co| {
                        let types = cache
                            .reader_at(co + 4)
                            .block()?
                            .elements(TYPE_SIZE)
                            .map(|to| {
                                Ok(WeaponType {
                                    label: sid(cache, to)?,
                                    actions: refs(cache, cache.reader_at(to + 4).block()?)?,
                                    overlays: refs(cache, cache.reader_at(to + 16).block()?)?,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(WeaponClass {
                            label: sid(cache, co)?,
                            types,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Mode {
                    label: sid(cache, mo)?,
                    classes,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let inheritance = block(0x80)?
            .elements(INHERITANCE_SIZE)
            .map(|o| {
                Ok(Inheritance {
                    graph: cache.reader_at(o).tag_ref()?.index,
                    root_z_offset: cache.reader_at(o + 40).f32()?,
                    flags: cache.reader_at(o + 44).u32()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let groups = block(0xF8)?
            .elements(GROUP_SIZE)
            .map(|o| {
                let mut r = cache.reader_at(o);
                Ok(ResourceGroup {
                    member_count: r.i32()?,
                    resource: ResourceId(r.u32()?),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Graph {
            tag: tag.index,
            name: tag.name().to_string(),
            parent,
            nodes,
            animations,
            modes,
            blend_screens,
            inheritance,
            groups,
        })
    }

    /// The graph of an object (`bipd`, `mach`, `vehi`, `scen`, ...) through
    /// its model (`hlmt`), or of a model directly. `None` when it has none.
    pub fn tag_of_object<'a>(cache: &'a CacheFile, object: &'a TagEntry) -> Result<Option<&'a TagEntry>> {
        let code = cache.tags.class_of(object).code.as_str().to_string();
        let hlmt = match code.as_str() {
            "jmad" => return Ok(Some(object)),
            "hlmt" => Some(object),
            _ => cache
                .reader_at(cache.tag_offset(object)? + 0x34)
                .tag_ref()?
                .index
                .and_then(|i| cache.tags.get(i)),
        };
        let Some(hlmt) = hlmt else { return Ok(None) };
        let jmad = cache.reader_at(cache.tag_offset(hlmt)? + 0x20).tag_ref()?;
        Ok(jmad.index.and_then(|i| cache.tags.get(i)))
    }

    /// Index of the animation called `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.animations.iter().position(|a| a.name == name)
    }

    /// Index of the graph node called `name`.
    pub fn node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// The action `mode:class:type:action`, with Halo's fallbacks: the
    /// exact weapon type, then `any`; the exact class, then `any`; the
    /// exact mode, then `combat`, then `any`. Returns the reference as the
    /// graph stores it; [`crate::Library::resolve`] follows inheritance.
    pub fn action(&self, mode: &str, class: &str, kind: &str, action: &str) -> Option<&AnimRef> {
        self.search(mode, class, kind, |t| t.actions.iter().find(|a| a.label == action))
    }

    /// Like [`Graph::action`], for an overlay (`fire_1`, `aim_still_up`).
    pub fn overlay(&self, mode: &str, class: &str, kind: &str, overlay: &str) -> Option<&AnimRef> {
        self.search(mode, class, kind, |t| t.overlays.iter().find(|a| a.label == overlay))
    }

    fn search<'g>(
        &'g self,
        mode: &str,
        class: &str,
        kind: &str,
        pick: impl Fn(&'g WeaponType) -> Option<&'g AnimRef>,
    ) -> Option<&'g AnimRef> {
        let modes = [mode, "combat", "any"];
        let classes = [class, "any"];
        let kinds = [kind, "any"];
        for m in modes {
            let Some(mo) = self.modes.iter().find(|x| x.label == m) else { continue };
            for c in classes {
                let Some(co) = mo.classes.iter().find(|x| x.label == c) else { continue };
                for k in kinds {
                    let Some(to) = co.types.iter().find(|x| x.label == k) else { continue };
                    if let Some(found) = pick(to) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// The variants of an animation (it and its siblings through the
    /// previous/next links), in order.
    pub fn variants(&self, index: usize) -> Vec<usize> {
        let mut first = index;
        let mut guard = 0;
        while let Some(p) = self.animations.get(first).and_then(|a| a.previous_variant) {
            first = p as usize;
            guard += 1;
            if guard > self.animations.len() {
                break;
            }
        }
        let mut out = vec![first];
        while let Some(n) = self.animations.get(*out.last().unwrap()).and_then(|a| a.next_variant) {
            if out.contains(&(n as usize)) || out.len() > self.animations.len() {
                break;
            }
            out.push(n as usize);
        }
        out
    }

    /// One variant chosen by weight, `roll` in `0..1`. Variants with
    /// weight 0 count as 1 when every weight is 0.
    pub fn pick_variant(&self, index: usize, roll: f32) -> usize {
        let all = self.variants(index);
        let weights: Vec<f32> = all
            .iter()
            .map(|&i| self.animations[i].weight.max(0.0))
            .collect();
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            let k = ((roll.clamp(0.0, 0.999_999)) * all.len() as f32) as usize;
            return all[k.min(all.len() - 1)];
        }
        let mut acc = 0.0;
        for (&i, w) in all.iter().zip(&weights) {
            acc += w / total;
            if roll < acc {
                return i;
            }
        }
        *all.last().unwrap()
    }
}
