//! `blam ai <map> [--squads] [--zones] [--characters]`: the scenario's AI.
//! With no option: the characters, then which squads (and how many actors
//! of which character) each zone set holds. `--squads` lists every squad
//! with its cells and starting locations, `--zones` the AI zones with their
//! areas, `--characters` each character's numbers in full.
//! `docs/format/ai.md` has the layouts.

use std::collections::BTreeMap;

use anyhow::Result;
use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_geometry::ai::{self, AiData, Character, Unit};

use crate::Args;

fn short(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

fn tag_name(cache: &CacheFile, i: Option<u32>) -> String {
    i.and_then(|i| cache.tags.get(i)).map_or_else(|| "-".to_string(), |t| short(t.name()).to_string())
}

fn team_name(t: i16) -> &'static str {
    match t {
        ai::team::DEFAULT => "default",
        ai::team::PLAYER => "player",
        ai::team::HUMAN => "human",
        ai::team::COVENANT => "covenant",
        ai::team::FLOOD => "flood",
        ai::team::SENTINEL => "sentinel",
        ai::team::HERETIC => "heretic",
        ai::team::PROPHET => "prophet",
        ai::team::GUILTY => "guilty",
        _ => "?",
    }
}

fn unit_of(cache: &CacheFile, c: &Character) -> Option<Unit> {
    let t = cache.tags.get(c.unit?)?;
    ai::read_unit(cache, t).ok()
}

fn print_character(cache: &CacheFile, i: usize, c: &Character, full: bool) {
    let unit = unit_of(cache, c);
    println!(
        "  [{i:2}] {}  (type {}, {} hp body / {} shield; legendary {} / {})",
        c.tag_path, c.actor_type, c.vitality.body, c.vitality.shield, c.vitality.legendary_body, c.vitality.legendary_shield
    );
    if let Some(u) = &unit {
        println!(
            "       unit {} {} team {} ({}), model {}, coll {}, jmad {}",
            u.class,
            short(&u.tag_path),
            u.team,
            team_name(u.team),
            tag_name(cache, u.render_model),
            tag_name(cache, u.collision_model),
            tag_name(cache, u.animations)
        );
        println!(
            "       body: height {:.3} / {:.3} wu, radius {:.3}, eye {:.3}, physics flags 0x{:X}{}; model damage info {} body, {} shield; melee {}; built-in weapons {}",
            u.height_standing,
            u.height_crouching,
            u.radius,
            u.eye_height,
            u.physics_flags,
            if u.flies { " (flies)" } else { "" },
            u.body_vitality,
            u.shield_vitality,
            tag_name(cache, u.melee_damage),
            if u.weapons.is_empty() {
                "-".to_string()
            } else {
                u.weapons.iter().map(|w| tag_name(cache, Some(*w))).collect::<Vec<_>>().join(", ")
            }
        );
    }
    if !full {
        return;
    }
    if !c.parents.is_empty() {
        println!("       parents: {}", c.parents.iter().map(|p| short(p)).collect::<Vec<_>>().join(" <- "));
    }
    let p = &c.perception;
    println!(
        "       sees {:.0} wu, central {:.0} deg, max {:.0} deg, peripheral {:.0} deg within {:.1} wu; hears {:.0} wu",
        p.max_vision_distance,
        p.central_vision_angle.to_degrees(),
        p.max_vision_angle.to_degrees(),
        p.peripheral_vision_angle.to_degrees(),
        p.peripheral_distance,
        p.hearing_distance
    );
    let m = &c.charge;
    println!(
        "       melee: considers within {:.1} wu (chance {:.2}), swings within {:.2}, gives up past {:.1}, timeout {:.1} s, delay {:.1} s, leap {:?} wu (chance {:.2}); pathfinding radius {:.2}",
        m.melee_consider_range,
        m.melee_chance,
        m.melee_attack_range,
        m.melee_abort_range,
        m.melee_attack_timeout,
        m.melee_attack_delay,
        m.leap_range,
        m.leap_chance,
        c.pathfinding_radius
    );
    for w in c.weapons.iter().filter(|w| w.weapon.is_some()) {
        let pattern = c.firing_pattern(w.weapon);
        println!(
            "       weapon {:<22} range {:.1}..{:.1}, combat {:.1}..{:.1} wu, first burst {:.1}..{:.1} s{}",
            short(&w.weapon_path),
            w.minimum_firing_range,
            w.maximum_firing_range,
            w.normal_combat_range[0],
            w.normal_combat_range[1],
            w.first_burst_delay[0],
            w.first_burst_delay[1],
            pattern.map_or(String::new(), |f| format!(
                "; fires {:.1}/s, bursts {:.1}..{:.1} s every {:.1}..{:.1} s, error {:.2} deg (max {:.1}), tracking {:.2}, leading {:.2}",
                f.rate_of_fire,
                f.burst_duration[0],
                f.burst_duration[1],
                f.burst_separation[0],
                f.burst_separation[1],
                f.projectile_error.to_degrees(),
                f.max_error_angle.to_degrees(),
                f.target_tracking,
                f.target_leading
            ))
        );
    }
}

fn print_squads(cache: &CacheFile, data: &AiData) {
    println!("\nsquads ({}):", data.squads.len());
    for s in &data.squads {
        println!(
            "  [{:3}] {:<30} group {:<24} zone {:<20} bsps 0x{:03X}",
            s.index,
            s.name,
            s.group.and_then(|g| data.squad_groups.get(g as usize)).map_or("-", |g| g.name.as_str()),
            s.zone.and_then(|z| data.zones.get(z as usize)).map_or("-", |z| z.name.as_str()),
            data.squad_bsps(s)
        );
        for c in &s.cells {
            println!(
                "        cell: {} x {} with {}{}{}{}",
                c.count,
                data.characters
                    .get(c.character.map_or(usize::MAX, usize::from))
                    .and_then(Option::as_ref)
                    .map_or("-", |ch| short(&ch.tag_path)),
                tag_name(cache, data.weapon_of(c, None)),
                c.vehicle
                    .and_then(|v| data.vehicle_palette.get(v as usize).copied().flatten())
                    .map_or(String::new(), |v| format!(", in a {}", tag_name(cache, Some(v)))),
                if c.placement_script.is_empty() { String::new() } else { format!(", script {}", c.placement_script) },
                if c.flags != 0 { format!(", flags {}", c.flags) } else { String::new() }
            );
            for l in &c.locations {
                let over = match (l.character, l.weapon) {
                    (None, None) => String::new(),
                    (ch, w) => format!(
                        "  as {} with {}",
                        ch.and_then(|i| data.characters.get(i as usize)).and_then(Option::as_ref).map_or("-", |c| short(&c.tag_path)),
                        tag_name(cache, w.and_then(|i| data.weapon_palette.get(i as usize).copied().flatten()))
                    ),
                };
                println!(
                    "          {:<22} ({:8.2}, {:8.2}, {:7.2}) bsp {:>2} yaw {:6.1}{}{}",
                    l.name,
                    l.position.x,
                    l.position.y,
                    l.position.z,
                    l.bsp.map_or("-".to_string(), |b| b.to_string()),
                    l.facing[0].to_degrees(),
                    if l.swarm > 0 { format!(" swarm {}", l.swarm) } else { String::new() },
                    over
                );
            }
        }
    }
}

fn print_zones(data: &AiData) {
    println!("\nAI zones ({}):", data.zones.len());
    for (i, z) in data.zones.iter().enumerate() {
        println!(
            "  [{i:2}] {:<22} bsp {:>2}: {} firing positions, {} areas",
            z.name,
            z.bsp.map_or("-".to_string(), |b| b.to_string()),
            z.firing_positions.len(),
            z.areas.len()
        );
        for a in &z.areas {
            println!(
                "        {:<16} centre ({:8.2}, {:8.2}, {:7.2}) radius {:5.2}, firing positions {}..{}",
                a.name,
                a.centre.x,
                a.centre.y,
                a.centre.z,
                a.radius,
                a.first_firing_position,
                a.first_firing_position + a.firing_position_count
            );
        }
    }
}

pub fn run(cache: &CacheFile, args: &Args) -> Result<()> {
    let data = ai::read(cache)?;
    let locations: usize = data.squads.iter().flat_map(|s| &s.cells).map(|c| c.locations.len()).sum();
    let cells: usize = data.squads.iter().map(|s| s.cells.len()).sum();
    let fps: usize = data.zones.iter().map(|z| z.firing_positions.len()).sum();
    println!(
        "{}: {} squad groups, {} squads, {} cells, {} starting locations, {} AI zones ({} firing positions), {} characters",
        cache.header.map_name,
        data.squad_groups.len(),
        data.squads.len(),
        cells,
        locations,
        data.zones.len(),
        fps,
        data.characters.iter().flatten().count()
    );
    println!("\ncharacters:");
    for (i, c) in data.characters.iter().enumerate() {
        match c {
            Some(c) => print_character(cache, i, c, args.has("characters")),
            None => println!("  [{i:2}] -"),
        }
    }

    // Which squads each zone set holds: those whose starting locations all
    // lie in its BSPs.
    let mut scene = Scene::default();
    blam_geometry::zones::decode(cache, &mut scene)?;
    if !scene.zone_sets.is_empty() {
        println!("\nby zone set (squads whose starting locations, or else AI zone, lie in its BSPs; actors per character):");
    }
    for (n, z) in scene.zone_sets.iter().enumerate() {
        let squads: Vec<_> = data
            .squads
            .iter()
            .filter(|s| data.squad_bsps(s) != 0 && data.squad_bsps(s) & !z.bsps == 0)
            .collect();
        let mut by_char: BTreeMap<String, usize> = BTreeMap::new();
        for s in &squads {
            for c in &s.cells {
                for (k, l) in c.locations.iter().enumerate().take(c.count.max(0) as usize) {
                    let _ = k;
                    let name = data.character_of(c, Some(l)).map_or("-", |ch| short(&ch.tag_path)).to_string();
                    *by_char.entry(name).or_default() += 1;
                }
            }
        }
        println!(
            "  zone set {n} {:<18} BSPs 0x{:03X}: {} squads; {}",
            z.name,
            z.bsps,
            squads.len(),
            by_char.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ")
        );
    }
    let unplaced: Vec<_> = data.squads.iter().filter(|s| data.squad_bsps(s) == 0).map(|s| s.name.as_str()).collect();
    if !unplaced.is_empty() {
        println!("  squads in no BSP: {}", unplaced.join(", "));
    }

    println!("\nsquad groups:");
    for (i, g) in data.squad_groups.iter().enumerate() {
        let members = data.squads_in_group(i).len();
        println!(
            "  [{i:2}] {:<24} parent {:<24} {} squads",
            g.name,
            g.parent.and_then(|p| data.squad_groups.get(p as usize)).map_or("-", |p| p.name.as_str()),
            members
        );
    }
    if args.has("zones") {
        print_zones(&data);
    }
    if args.has("squads") {
        print_squads(cache, &data);
    }
    Ok(())
}
