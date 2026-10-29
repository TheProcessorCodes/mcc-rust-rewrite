//! `blam weapons <map> [--all] [--meshes] [--markers]`: the map's weapons,
//! their numbers and markers, the Spartan's hand markers, and where the
//! scenario places weapons and equipment.

use std::collections::BTreeMap;

use anyhow::Result;
use blam_cache::CacheFile;
use blam_geometry::weapons::{self, ItemKind, Marker, Weapon};

use crate::Args;

fn marker_line(m: &Marker) -> String {
    let w = m.model.w_axis;
    format!(
        "{:<20} node {:>2} {:<10} local ({:.4}, {:.4}, {:.4}) q ({:.3}, {:.3}, {:.3}, {:.3})  model ({:.4}, {:.4}, {:.4})",
        m.name,
        m.node,
        m.node_name,
        m.position.x,
        m.position.y,
        m.position.z,
        m.rotation.x,
        m.rotation.y,
        m.rotation.z,
        m.rotation.w,
        w.x,
        w.y,
        w.z
    )
}

fn short(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

fn print_weapon(w: &Weapon, show_markers: bool) {
    println!(
        "#{} {} ({} {}), holster {}",
        w.tag_index,
        w.tag_path,
        w.class,
        w.short_name,
        if w.holster_marker.is_empty() { "-" } else { &w.holster_marker }
    );
    println!(
        "    model {}  zoom {} level(s) {:?}",
        if w.model.is_empty() { "-" } else { &w.model },
        w.zoom_levels,
        w.zoom_range
    );
    for fp in &w.first_person {
        println!("    first person {}  ({})", fp.render_model_path, fp.animations);
    }
    for m in &w.magazines {
        println!(
            "    magazine {} rounds, {} at spawn, {} max, {} per reload",
            m.rounds_loaded_maximum, m.rounds_total_initial, m.rounds_total_maximum, m.rounds_reloaded
        );
    }
    for (i, t) in w.triggers.iter().enumerate() {
        println!(
            "    trigger {i}: behaviour {} barrel {:?} secondary {:?}",
            t.behavior, t.primary_barrel, t.secondary_barrel
        );
    }
    for (i, b) in w.barrels.iter().enumerate() {
        println!(
            "    barrel {i}: {:?} rounds/s, {:?} shots per pull, recovery {:.2} s, magazine {:?}, {} round(s) per shot, error {:.2}..{:.2} deg",
            b.rounds_per_second,
            b.shots_per_fire,
            b.fire_recovery_time,
            b.magazine,
            b.rounds_per_shot,
            b.error_angle[0].to_degrees(),
            b.error_angle[1].to_degrees()
        );
        if let Some(p) = &b.projectile {
            print!(
                "        fires {} at {}..{} wu/s{}",
                short(&p.tag),
                p.initial_velocity,
                p.final_velocity,
                if p.is_near_hitscan() { " (near hitscan)" } else { "" }
            );
            match &p.damage {
                Some(d) => println!(
                    ", damage {} / {:?}, radius {:?} ({})",
                    d.lower_bound,
                    d.upper_bound,
                    d.radius,
                    short(&d.tag)
                ),
                None => println!(),
            }
        }
    }
    if show_markers {
        for m in &w.markers {
            println!("    marker {}", marker_line(m));
        }
    } else {
        let names: Vec<&str> = {
            let mut v: Vec<&str> = w.markers.iter().map(|m| m.name.as_str()).collect();
            v.dedup();
            v
        };
        println!("    markers: {}", names.join(", "));
    }
}

pub fn run(cache: &CacheFile, args: &Args) -> Result<()> {
    let all = weapons::weapons(cache)?;
    let placements = weapons::item_placements(cache)?;
    let placed: BTreeMap<u32, usize> = placements
        .iter()
        .filter(|p| p.kind == ItemKind::Weapon)
        .filter_map(|p| p.tag)
        .fold(BTreeMap::new(), |mut m, t| {
            *m.entry(t).or_default() += 1;
            m
        });
    let shown: Vec<&Weapon> = all
        .iter()
        .filter(|w| args.has("all") || placed.contains_key(&w.tag_index))
        .collect();
    println!(
        "{}: {} weap tags, {} placed on the map ({} placements)",
        cache.header.map_name,
        all.len(),
        placed.len(),
        placed.values().sum::<usize>()
    );
    for w in &shown {
        println!();
        print_weapon(w, args.has("markers"));
    }

    if args.has("meshes") {
        println!();
        let resources = cache.resources()?;
        for w in &shown {
            let models = w
                .render_model
                .map(|m| ("third person", m))
                .into_iter()
                .chain(w.first_person.first().and_then(|f| f.render_model).map(|m| ("first person", m)));
            for (label, mode) in models {
                let mut scene = blam_geometry::Scene::default();
                // A material table indexes one scene; a fresh scene needs a fresh table.
                let mut materials = blam_geometry::materials::MaterialTable::default();
                match weapons::decode_model(cache, &resources, &mut materials, mode, &mut scene) {
                    Ok(parts) => println!(
                        "{} {label}: {} pieces, {} triangles, {} materials, {} bones",
                        short(&w.tag_path),
                        parts.pieces.len(),
                        scene.triangle_count_of_meshes(),
                        scene.materials.len(),
                        parts.bones.len()
                    ),
                    Err(e) => println!("{} {label}: not decoded: {e}", short(&w.tag_path)),
                }
            }
        }
        resources.clear_page_cache();
    }

    println!();
    match weapons::spartan_markers(cache)? {
        Some(markers) => {
            println!("Spartan ({}) hand and holster markers:", weapons::SPARTAN_MODEL);
            for m in markers.iter().filter(|m| {
                matches!(
                    m.name.as_str(),
                    "right_hand" | "left_hand" | "weapon_back" | "weapon_thigh"
                )
            }) {
                println!("    {}", marker_line(m));
            }
        }
        None => println!("no {} on this map", weapons::SPARTAN_MODEL),
    }

    println!();
    println!("placements (position wu, yaw/pitch/roll deg, respawn s):");
    for p in &placements {
        let [y, pi, r] = p.yaw_pitch_roll.map(f32::to_degrees);
        println!(
            "  {:<9} {:>3} {:<24} ({:8.2}, {:8.2}, {:7.2})  ({:7.1}, {:6.1}, {:6.1})  {:>3}",
            format!("{:?}", p.kind).to_lowercase(),
            p.index,
            short(&p.tag_path),
            p.position.x,
            p.position.y,
            p.position.z,
            y,
            pi,
            r,
            p.respawn_seconds
        );
    }
    Ok(())
}
