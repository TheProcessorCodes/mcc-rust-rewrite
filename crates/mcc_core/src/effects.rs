//! Effects asked for by the scripts (and anyone else): `mcc_script`
//! queues an [`EffectRequest`] for each `effect_new*` call and each damage
//! section's effect; `mcc_effects` plays them from the map's `effe` tags.

use bevy::prelude::*;

/// One effect to play.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectRequest {
    /// The `effe` tag's path, as the scripts name it.
    pub name: String,
    /// Where, Halo world units.
    pub at: Vec3,
    /// The effect's frame in the Halo world: x forward, y left, z up.
    /// Particles leave along z, parts' velocities are turned from x.
    pub frame: Mat3,
    /// The tag of the object the effect comes from (a damaged plate), so
    /// its own stand-in (the plate falling) is not made twice.
    pub source: Option<u32>,
}

impl EffectRequest {
    /// An effect at a point, upright.
    pub fn at(name: impl Into<String>, at: Vec3) -> EffectRequest {
        EffectRequest {
            name: name.into(),
            at,
            frame: Mat3::IDENTITY,
            source: None,
        }
    }
}

/// Effects waiting to start, oldest first.
#[derive(Resource, Default, Debug)]
pub struct EffectQueue(pub Vec<EffectRequest>);

impl EffectQueue {
    /// Most kept while nobody plays them.
    pub const LIMIT: usize = 256;

    pub fn push(&mut self, r: EffectRequest) {
        if self.0.len() >= Self::LIMIT {
            self.0.remove(0);
        }
        self.0.push(r);
    }
}
