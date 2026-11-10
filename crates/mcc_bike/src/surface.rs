//! What the ground is made of, for the tyres: Halo's collision surfaces
//! sorted into the kinds of ground Reflex's tracks have, each with Reflex's
//! grip and drag indices.
//!
//! Reflex paints every track with a friction map: per spot a grip class 0..5,
//! which becomes a grip index 0.1 / 0.3 / 0.5 / 0.65 / 0.8 / 0.9 (0 sticky,
//! 1 slippery: it blends the tyres' Sticky, Normal and Slippery values), and a
//! drag class 0..3, which becomes a drag index 0 / 0.25 / 0.75 / 1 (the
//! terrain drag that bogs the bike down in sand or mud). The two wheels'
//! samples are averaged and both tyres get the mean (`reflex_core::tyre`,
//! "Surface index" in the Reflex physics notes). Its stock dirt is grip 0.48,
//! drag 0.25.
//!
//! A Halo 3 collision surface names its shader, and the shader the global
//! material it is made of (`tough_terrain_dirt`, `soft_terrain_snow`,
//! `hard_metal_thick` ...; `blam_geometry::CollisionMaterial`). The kind of
//! ground is read from those names, else from words in the shader's path
//! (the other Halos, which name no global material here). A terrain shader
//! has four layers: when its path names one of them that is the kind
//! (`riverworld_ground_snow`: dirt, grass, dirt, snow, and "snow" in its
//! path), else the most common of them.
//!
//! The Reflex model gets the indices as they are. The arcade models scale
//! their tyre grip by Reflex's static grip at that index over its grip on
//! plain ground, and slow the bike by its terrain drag.

use blam_geometry::CollisionMaterial;
use reflex_bevy::reflex_core::curve::blend3;

/// A kind of ground, with Reflex's grip and drag classes for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Surface {
    /// Dirt, and anything not known: Reflex's stock free-ride dirt.
    #[default]
    Dirt,
    Grass,
    Gravel,
    Sand,
    Snow,
    Mud,
    Ice,
    /// Shallow water over the ground.
    Water,
    /// Rock, concrete, pavement: hard and grippy, no drag.
    Stone,
    Metal,
    Wood,
    Glass,
    /// Flood growth.
    Flesh,
    Rubber,
    /// Plastic, cloth, carpet and other man-made floors.
    Floor,
}

impl Surface {
    /// Reflex's grip index: 0 sticky .. 1 slippery.
    pub fn grip_index(self) -> f64 {
        const CLASS: [f64; 6] = [0.1, 0.3, 0.5, 0.65, 0.8, 0.9];
        match self {
            // Reflex's own dirt sits between classes 1 and 2.
            Surface::Dirt => 0.48,
            Surface::Rubber => CLASS[0],
            Surface::Stone => CLASS[1],
            Surface::Wood | Surface::Floor => CLASS[2],
            Surface::Grass | Surface::Gravel | Surface::Sand | Surface::Water => CLASS[3],
            Surface::Snow | Surface::Mud | Surface::Metal | Surface::Flesh => CLASS[4],
            Surface::Ice | Surface::Glass => CLASS[5],
        }
    }

    /// Reflex's drag index: 0 none .. 1 most.
    pub fn drag_index(self) -> f64 {
        const CLASS: [f64; 4] = [0.0, 0.25, 0.75, 1.0];
        match self {
            Surface::Stone
            | Surface::Metal
            | Surface::Wood
            | Surface::Glass
            | Surface::Ice
            | Surface::Rubber
            | Surface::Floor => CLASS[0],
            Surface::Dirt | Surface::Grass | Surface::Gravel => CLASS[1],
            Surface::Sand | Surface::Snow | Surface::Flesh => CLASS[2],
            Surface::Mud | Surface::Water => CLASS[3],
        }
    }

    /// Tyre grip for the arcade models, as a share of their grip on dirt:
    /// Reflex's stock static grip at this grip index (Sticky 1.5, Normal
    /// 1.3, Slippery 1.0, blended as `reflex_core::curve::blend3` does) over
    /// its value on dirt, so dirt rides as the arcade models always did.
    pub fn arcade_grip(self) -> f32 {
        let mu = |s: Surface| blend3(&[1.5, 1.3, 1.0], s.grip_index());
        (mu(self) / mu(Surface::Dirt)) as f32
    }

    /// Terrain drag for the arcade models, as a share of the tyre's load
    /// that holds the bike back along the way it rolls: Reflex's stock
    /// `TerrainDrag` at this drag index (Sticky 0, Normal 5, Slippery 30)
    /// beyond its value on dirt, scaled so the deepest (mud, water) slows
    /// the bike by about 0.1 g. Dirt and harder ground add nothing, so the
    /// arcade models ride dirt as before.
    pub fn arcade_drag(self) -> f32 {
        let drag = |s: Surface| blend3(&[0.0, 5.0, 30.0], s.drag_index());
        ((drag(self) - drag(Surface::Dirt)).max(0.0) / 30.0 * 0.1) as f32
    }

    /// The kind of ground a collision material is.
    pub fn of(material: &CollisionMaterial) -> Surface {
        let path = Surface::from_name(&material.shader.to_ascii_lowercase());
        let mut counts: Vec<(Surface, usize)> = Vec::new();
        for name in &material.global_materials {
            let Some(s) = Surface::from_name(name) else {
                continue;
            };
            match counts.iter_mut().find(|(k, _)| *k == s) {
                Some((_, n)) => *n += 1,
                None => counts.push((s, 1)),
            }
        }
        if let Some(p) = path
            && counts.iter().any(|(s, _)| *s == p)
        {
            return p;
        }
        // The first of the most common (`max_by_key` keeps the last).
        let most = counts.iter().map(|(_, n)| *n).max();
        counts
            .iter()
            .find(|(_, n)| Some(*n) == most)
            .map(|(s, _)| *s)
            .or(path)
            .unwrap_or_default()
    }

    /// The kind of ground a global material name or a shader path speaks
    /// of, by the first word it has: the more particular words first
    /// (`hard_terrain_ice` is ice, `liquid_thin_water` water).
    pub fn from_name(name: &str) -> Option<Surface> {
        const WORDS: [(&str, Surface); 30] = [
            ("ice", Surface::Ice),
            ("glass", Surface::Glass),
            ("snow", Surface::Snow),
            ("mud", Surface::Mud),
            ("sand", Surface::Sand),
            ("beach", Surface::Sand),
            ("water", Surface::Water),
            ("liquid", Surface::Water),
            ("gravel", Surface::Gravel),
            ("grass", Surface::Grass),
            ("underbrush", Surface::Grass),
            ("plant", Surface::Grass),
            ("moss", Surface::Grass),
            ("floodflesh", Surface::Flesh),
            ("flesh", Surface::Flesh),
            ("dirt", Surface::Dirt),
            ("rubber", Surface::Rubber),
            ("metal", Surface::Metal),
            ("grate", Surface::Metal),
            ("grating", Surface::Metal),
            ("concrete", Surface::Stone),
            ("stone", Surface::Stone),
            ("rock", Surface::Stone),
            ("cliff", Surface::Stone),
            ("asphalt", Surface::Stone),
            ("pavement", Surface::Stone),
            ("wood", Surface::Wood),
            ("carpet", Surface::Floor),
            ("plastic", Surface::Floor),
            ("cloth", Surface::Floor),
        ];
        let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric()).collect();
        // Whole words first (`rock`, not the `rock` in `bedrock_...`), then
        // words inside longer ones (`riverworld_ground_snow` has "snow").
        WORDS
            .iter()
            .find(|(w, _)| words.contains(w))
            .or_else(|| WORDS.iter().find(|(w, _)| w.len() >= 4 && name.contains(w)))
            .map(|(_, s)| *s)
            .or_else(|| {
                // Halo 3's own families with no particular word: hard
                // terrain is rock, soft and tough terrain dirt.
                if name.starts_with("hard_terrain") {
                    Some(Surface::Stone)
                } else if name.starts_with("soft_terrain") || name.starts_with("tough_terrain") {
                    Some(Surface::Dirt)
                } else if name.starts_with("energy_") {
                    Some(Surface::Metal)
                } else if name.starts_with("soft_inorganic") || name.starts_with("tough_inorganic") {
                    Some(Surface::Floor)
                } else {
                    None
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(shader: &str, globals: &[&str]) -> CollisionMaterial {
        CollisionMaterial {
            shader: shader.into(),
            shader_class: "rmsh".into(),
            global_materials: globals.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn halo_3_materials_are_read_by_name() {
        for (name, kind) in [
            ("tough_terrain_dirt", Surface::Dirt),
            ("tough_terrain_grass", Surface::Grass),
            ("soft_terrain_snow", Surface::Snow),
            ("soft_terrain_sand", Surface::Sand),
            ("soft_terrain_beach", Surface::Sand),
            ("tough_terrain_mud", Surface::Mud),
            ("tough_terrain_gravel", Surface::Gravel),
            ("hard_terrain_ice", Surface::Ice),
            ("hard_terrain_stone_rough", Surface::Stone),
            ("hard_terrain_concrete", Surface::Stone),
            ("hard_terrain", Surface::Stone),
            ("hard_metal_thick_for", Surface::Metal),
            ("hard_metal_thin_hum_grating", Surface::Metal),
            ("tough_organic_wood", Surface::Wood),
            ("tough_terrain_woodfloor_docks", Surface::Wood),
            ("liquid_thin_water_shallow", Surface::Water),
            ("tough_terrain_floodflesh_wet", Surface::Flesh),
            ("hard_terrain_glass_unbreakable", Surface::Glass),
            ("tough_inorganic_rubber", Surface::Rubber),
            ("soft_organic_plant_leafy", Surface::Grass),
            ("energy_shield_thick_for", Surface::Metal),
        ] {
            assert_eq!(Surface::from_name(name), Some(kind), "{name}");
        }
        assert_eq!(Surface::from_name("special_death"), None);
    }

    #[test]
    fn a_terrain_shader_is_its_most_common_layer_or_its_path() {
        let snow = material(
            "levels\\multi\\riverworld\\shaders\\riverworld_ground_snow",
            &["tough_terrain_dirt", "tough_terrain_grass", "tough_terrain_dirt", "soft_terrain_snow"],
        );
        assert_eq!(Surface::of(&snow), Surface::Snow, "the path names one of its layers");
        let dirt = material(
            "levels\\multi\\riverworld\\shaders\\riverworld_ground",
            &["tough_terrain_dirt", "tough_terrain_grass", "tough_terrain_dirt", "soft_terrain_snow"],
        );
        assert_eq!(Surface::of(&dirt), Surface::Dirt);
        let metal = material("levels\\multi\\riverworld\\shaders\\riverworld_metal", &["hard_metal_thick_for"]);
        assert_eq!(Surface::of(&metal), Surface::Metal);
        // No global material (Halo CE, Halo 2): the shader's path.
        assert_eq!(Surface::of(&material("levels\\a50\\shaders\\snow_ground", &[])), Surface::Snow);
        assert_eq!(Surface::of(&material("levels\\b30\\shaders\\metal plate", &[])), Surface::Metal);
        assert_eq!(Surface::of(&material("levels\\b30\\shaders\\pathway", &[])), Surface::Dirt);
    }

    #[test]
    fn slippery_ground_grips_less_and_soft_ground_drags() {
        assert_eq!(Surface::Dirt.arcade_grip(), 1.0);
        assert_eq!(Surface::Dirt.arcade_drag(), 0.0);
        assert!(Surface::Stone.arcade_grip() > Surface::Dirt.arcade_grip());
        assert!(Surface::Snow.arcade_grip() < Surface::Dirt.arcade_grip());
        assert!(Surface::Metal.arcade_grip() < Surface::Dirt.arcade_grip());
        assert!(Surface::Ice.arcade_grip() < Surface::Snow.arcade_grip());
        assert_eq!(Surface::Stone.arcade_drag(), 0.0);
        assert!(Surface::Sand.arcade_drag() > Surface::Dirt.arcade_drag());
        assert!(Surface::Mud.arcade_drag() > Surface::Sand.arcade_drag());
    }
}
