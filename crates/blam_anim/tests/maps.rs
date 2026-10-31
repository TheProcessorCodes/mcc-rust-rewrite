//! Reads the user's own maps and pins what the animation reader finds
//! (`docs/format/animation.md` section 11). Skips quietly when a map is not
//! installed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_anim::{AnimKind, Clip, Graph, Library, MovementKind, NodeMap, Pose, Skeleton};
use blam_cache::CacheFile;

fn open(name: &str) -> Option<CacheFile> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join(format!("{name}.map"));
    p.exists().then(|| CacheFile::open(p).expect("map opens"))
}

const FLOOD: &str = r"objects\characters\floodcombat_human\floodcombat_human";
const SENTINEL: &str = r"objects\characters\sentinel_aggressor\sentinel_aggressor";
const CHIEF: &str = r"objects\characters\masterchief\masterchief";

fn graph_of(cache: &CacheFile, lib: &mut Library, object: &str) -> u32 {
    let bipd = cache.tags.find("bipd", object).expect("biped");
    let jmad = Graph::tag_of_object(cache, bipd).unwrap().expect("graph");
    lib.load(cache, jmad.index).unwrap();
    jmad.index
}

#[test]
fn every_animation_of_120_halo_decodes() {
    let Some(cache) = open("120_halo") else { return };
    let resources = cache.resources().unwrap();
    let mut codecs: BTreeMap<(bool, u8), usize> = BTreeMap::new();
    let (mut graphs, mut ok, mut no_data, mut other) = (0, 0, 0, Vec::new());
    for tag in cache.tags.of_class("jmad") {
        let g = Graph::read(&cache, tag).unwrap();
        graphs += 1;
        for i in 0..g.animations.len() {
            match Clip::load(&resources, &g, i) {
                Ok(c) => {
                    ok += 1;
                    if let Some(k) = c.static_codec {
                        *codecs.entry((false, k)).or_default() += 1;
                    }
                    if let Some(k) = c.animated_codec {
                        *codecs.entry((true, k)).or_default() += 1;
                    }
                }
                Err(e) if e.to_string().contains("not in this map") || e.to_string().contains("stale") => {
                    no_data += 1
                }
                Err(e) => other.push(format!("{} #{i}: {e}", g.name)),
            }
        }
        resources.clear_page_cache();
    }
    assert!(other.is_empty(), "{other:?}");
    assert_eq!((graphs, ok, no_data), (170, 5881, 115));
    let want: BTreeMap<(bool, u8), usize> = [
        ((false, 1), 4300),
        ((true, 3), 2830),
        ((true, 4), 1472),
        ((true, 5), 1),
        ((true, 6), 1131),
        ((true, 7), 7),
        ((true, 8), 440),
    ]
    .into_iter()
    .collect();
    assert_eq!(codecs, want);
}

#[test]
fn flood_combat_form_graph_and_poses() {
    let Some(cache) = open("120_halo") else { return };
    let mut lib = Library::default();
    let tag = graph_of(&cache, &mut lib, FLOOD);
    let g = lib.graph(tag).unwrap();
    assert_eq!(
        (g.nodes.len(), g.animations.len(), g.modes.len(), g.groups.len(), g.blend_screens.len()),
        (34, 150, 4, 16, 1)
    );
    // Halo's fallbacks: no rifle class, so `any`.
    let walk = lib.action(tag, "combat", "rifle", "ar", "move_front").unwrap();
    assert_eq!(walk.index, 42);
    let a = &g.animations[42];
    assert_eq!(a.name, "combat:any:any:move_front:var1");
    assert_eq!((a.kind, a.frame_count, a.movement), (AnimKind::Base, 24, MovementKind::DxDy));
    assert_eq!(g.variants(42), vec![42, 43]);

    let resources = cache.resources().unwrap();
    let clip = Clip::load(&resources, g, 42).unwrap();
    assert_eq!((clip.static_codec, clip.animated_codec), (Some(1), Some(3)));
    assert_eq!(clip.movement.len(), 24);
    let m = clip.motion_until(24.0);
    assert!((m.dx - 1.525).abs() < 0.01 && m.dy.abs() < 0.01, "{m:?}");

    // The render model's rest pose and the idle's first frame agree
    // closely: same node convention, same quaternion order.
    let mode = cache.tags.find("mode", FLOOD).unwrap();
    let skel = Skeleton::from_render_model(&cache, mode).unwrap();
    let map = NodeMap::by_name(g, &skel);
    assert_eq!(map.matched(), 34);
    let idle = lib.action(tag, "combat", "any", "any", "idle").unwrap();
    let clip = Clip::load(&resources, g, idle.index as usize).unwrap();
    let mut pose = Pose::rest(&skel);
    pose.apply(&clip, 0.0, &map, 1.0);
    for (gi, track) in clip.tracks.iter().enumerate() {
        if track.rotation.is_none() {
            continue;
        }
        let n = map.0[gi].unwrap() as usize;
        let d = pose.local[n].rotation.dot(skel.rest[n].rotation).abs();
        assert!(d > 0.8, "{}: {d}", g.nodes[gi].name);
    }
    // Model-space matrices: the head sits above the pelvis.
    let model = pose.model(&skel);
    let z = |name: &str| model[skel.index(name).unwrap()].w_axis.z;
    assert!(z("head") > z("pelvis") + 0.1);
}

#[test]
fn sentinel_idle_and_aim_grid() {
    let Some(cache) = open("120_halo") else { return };
    let mut lib = Library::default();
    let tag = graph_of(&cache, &mut lib, SENTINEL);
    let g = lib.graph(tag).unwrap();
    assert_eq!((g.nodes.len(), g.animations.len(), g.modes.len()), (23, 20, 4));
    let idle = lib.action(tag, "combat", "any", "any", "idle").unwrap();
    assert_eq!(g.animations[idle.index as usize].frame_count, 60);
    let aim = lib.overlay(tag, "combat", "any", "any", "aim_still_up").unwrap();
    let resources = cache.resources().unwrap();
    let clip = Clip::load(&resources, g, aim.index as usize).unwrap();
    assert_eq!((clip.kind, clip.frame_count, clip.animated_codec), (AnimKind::Overlay, 9, Some(8)));
    // The centre of the 3 x 3 aim grid is (nearly) the reference: no
    // change, except one node turned 5 degrees.
    let angles: Vec<f32> = clip
        .tracks
        .iter()
        .filter_map(|t| t.rotation.sample(4.0))
        .map(|q| 2.0 * q.w.abs().min(1.0).acos().to_degrees())
        .collect();
    assert_eq!(angles.len(), 4);
    assert_eq!(angles.iter().filter(|&&a| a > 0.1).count(), 1, "{angles:?}");
    assert!(angles.iter().all(|&a| a < 6.0), "{angles:?}");
    let screen = &g.blend_screens[0];
    assert_eq!((screen.yaw_steps, screen.pitch_steps), ((1, 1), (1, 1)));
}

#[test]
fn master_chief_graph() {
    let Some(cache) = open("120_halo") else { return };
    let mut lib = Library::default();
    let tag = graph_of(&cache, &mut lib, CHIEF);
    let g = lib.graph(tag).unwrap();
    assert_eq!((g.nodes.len(), g.animations.len(), g.modes.len(), g.groups.len()), (51, 730, 21, 190));
    let run = lib.action(tag, "combat", "rifle", "ar", "move_front").unwrap();
    assert_eq!(g.animations[run.index as usize].name, "combat:rifle:move_front");
    let resources = cache.resources().unwrap();
    let clip = Clip::load(&resources, g, run.index as usize).unwrap();
    assert_eq!(clip.frame_count, 20);
    // About Halo's run speed: 0.077 wu a frame, 2.3 wu a second.
    let speed = clip.motion_until(19.0).dx / 19.0 * blam_anim::FPS;
    assert!((speed - 2.3).abs() < 0.15, "{speed}");
    let mode = cache.tags.find("mode", CHIEF).unwrap();
    let skel = Skeleton::from_render_model(&cache, mode).unwrap();
    assert_eq!(NodeMap::by_name(g, &skel).matched(), 51);
}

#[test]
fn device_machines_have_position_animations() {
    let Some(cache) = open("120_halo") else { return };
    let mut lib = Library::default();
    let mach = cache.tags.find("mach", r"objects\levels\solo\120_halo\plates\island_test\island_test").unwrap();
    let jmad = Graph::tag_of_object(&cache, mach).unwrap().unwrap();
    lib.load(&cache, jmad.index).unwrap();
    let id = lib.named(jmad.index, "device:position").unwrap();
    let g = lib.graph(jmad.index).unwrap();
    assert_eq!(g.animations[id.index as usize].frame_count, 14);
    let resources = cache.resources().unwrap();
    let clip = Clip::load(&resources, g, id.index as usize).unwrap();
    assert_eq!(blam_anim::frame_at_position(&clip, 1.0), 13.0);
}

#[test]
fn inherited_graphs_resolve() {
    let Some(cache) = open("120_halo") else { return };
    let mut lib = Library::default();
    let dervish = cache.tags.find("jmad", r"objects\characters\dervish\dervish").unwrap();
    lib.load(&cache, dervish.index).unwrap();
    let g = lib.graph(dervish.index).unwrap();
    assert_eq!(g.inheritance.len(), 2);
    let elite = cache.tags.find("jmad", r"objects\characters\elite\elite").unwrap();
    // Some action of the dervish points into the elite's graph.
    let mut into_parent = 0;
    for m in &g.modes {
        for c in &m.classes {
            for t in &c.types {
                for a in t.actions.iter().chain(&t.overlays) {
                    let id = lib.resolve(dervish.index, a).expect("resolves");
                    if id.graph == elite.index {
                        into_parent += 1;
                    }
                }
            }
        }
    }
    assert!(into_parent > 0);
}

#[test]
fn riverworld_decodes() {
    let Some(cache) = open("riverworld") else { return };
    let resources = cache.resources().unwrap();
    let mut ok = 0;
    for tag in cache.tags.of_class("jmad") {
        let g = Graph::read(&cache, tag).unwrap();
        ok += (0..g.animations.len()).filter(|&i| Clip::load(&resources, &g, i).is_ok()).count();
        resources.clear_page_cache();
    }
    assert_eq!(ok, 3522);
}
