//! `blam anim <map> <jmad | object tag>`: an animation graph's skeleton,
//! animations and mode tree; `--anim NAME|N [--frame F]` decodes one
//! animation and prints a sampled pose (local and model space, with the
//! render model's rest pose beside it); `--action mode:class:type:action`
//! looks an action up with Halo's fallbacks; `--check` decodes every
//! animation of the graph (or of every graph with `--all`).

use anyhow::{Context, Result, bail};

use blam_anim::{AnimKind, Clip, Graph, Library, NodeMap, Pose, Skeleton};
use blam_cache::{CacheFile, TagEntry};

use crate::{Args, find_tag, usage};

pub fn run(cache: &CacheFile, args: &Args) -> Result<()> {
    if args.has("all") {
        return check_all(cache);
    }
    let Some(what) = args.positional.get(1) else {
        usage()
    };
    let tag = find_tag(cache, what)?;
    let jmad = Graph::tag_of_object(cache, tag)?
        .with_context(|| format!("{} has no animation graph", tag.name()))?;
    let mut lib = Library::default();
    lib.load(cache, jmad.index)?;
    let graph = lib.graph(jmad.index).unwrap();
    let resources = cache.resources()?;

    if args.has("check") {
        let (ok, failed) = check_graph(&resources, graph, true);
        println!("{}: {ok} decoded, {failed} not", graph.name);
        return Ok(());
    }

    if let Some(action) = args.flag("action") {
        let parts: Vec<&str> = action.split(':').collect();
        let [mode, class, kind, act] = parts[..] else {
            bail!("--action wants mode:class:type:action")
        };
        let id = lib
            .action(graph.tag, mode, class, kind, act)
            .or_else(|| lib.overlay(graph.tag, mode, class, kind, act))
            .with_context(|| format!("no action or overlay {action}"))?;
        let g = lib.graph(id.graph).unwrap();
        let variants = g.variants(id.index as usize);
        println!(
            "{action} -> {} #{} {:?} (variants {:?})",
            g.name,
            id.index,
            g.animations[id.index as usize].name,
            variants.iter().map(|&i| &g.animations[i].name).collect::<Vec<_>>()
        );
        return Ok(());
    }

    if let Some(which) = args.flag("anim") {
        let index = match which.parse::<usize>() {
            Ok(i) => i,
            Err(_) => graph.find(which).with_context(|| format!("no animation {which}"))?,
        };
        let frame: f32 = args.flag("frame").map_or(Ok(0.0), str::parse)?;
        return pose(cache, &resources, tag, graph, index, frame);
    }

    summary(graph);
    Ok(())
}

fn summary(graph: &Graph) {
    println!(
        "{} (#{}): {} nodes, {} animations, {} modes, {} blend screens, {} inherited graphs, {} resource groups",
        graph.name,
        graph.tag,
        graph.nodes.len(),
        graph.animations.len(),
        graph.modes.len(),
        graph.blend_screens.len(),
        graph.inheritance.len(),
        graph.groups.len()
    );
    println!("nodes:");
    for (i, n) in graph.nodes.iter().enumerate() {
        println!("  {i:3} {:24} parent {:?}", n.name, n.parent);
    }
    println!("animations (index, name, kind, frames, movement, variants):");
    for (i, a) in graph.animations.iter().enumerate() {
        println!(
            "  {i:4} {:48} {:11} {:4} frames  {:?}{}",
            a.name,
            a.kind.name(),
            a.frame_count,
            a.movement,
            if a.next_variant.is_some() || a.previous_variant.is_some() {
                format!("  weight {}", a.weight)
            } else {
                String::new()
            }
        );
    }
    println!("modes:");
    for m in &graph.modes {
        for c in &m.classes {
            for t in &c.types {
                let acts: Vec<String> = t
                    .actions
                    .iter()
                    .map(|a| match a.graph {
                        None => format!("{}={}", a.label, a.animation),
                        Some(g) => format!("{}=g{g}:{}", a.label, a.animation),
                    })
                    .collect();
                let ovs: Vec<&str> = t.overlays.iter().map(|a| a.label.as_str()).collect();
                println!("  {}:{}:{}  actions [{}]  overlays [{}]", m.label, c.label, t.label, acts.join(" "), ovs.join(" "));
            }
        }
    }
}

fn pose(
    cache: &CacheFile,
    resources: &blam_cache::Resources<'_>,
    object: &TagEntry,
    graph: &Graph,
    index: usize,
    frame: f32,
) -> Result<()> {
    let a = &graph.animations[index];
    let raw = blam_anim::codec::raw(resources, graph, index)?;
    let clip = Clip::decode(graph, index, &raw.member, raw.bytes())?;
    println!(
        "#{index} {}: {} {} frames, codecs static {:?} animated {:?}, {} data bytes, movement {:?} ({} frames)",
        a.name,
        a.kind.name(),
        clip.frame_count,
        clip.static_codec,
        clip.animated_codec,
        raw.member.data_size,
        clip.movement_kind,
        clip.movement.len()
    );
    if !clip.movement.is_empty() {
        let m = clip.motion_until(clip.frame_count as f32);
        println!(
            "  root motion over the clip: dx {:.3} dy {:.3} dz {:.3} wu, dyaw {:.1} deg",
            m.dx,
            m.dy,
            m.dz,
            m.dyaw.to_degrees()
        );
    }
    // The render model beside the graph, for the rest pose.
    let mode = match cache.tags.class_of(object).code.as_str() {
        "jmad" => cache.tags.find("mode", object.name()),
        _ => blam_geometry::scenario::render_model_of(cache, object)?,
    };
    let skeleton = match mode {
        Some(m) => Skeleton::from_render_model(cache, m)?,
        None => Skeleton::new(graph.nodes.iter().map(|n| {
            (n.name.clone(), n.parent, glam::Quat::IDENTITY, glam::Vec3::ZERO)
        })),
    };
    let map = NodeMap::by_name(graph, &skeleton);
    println!(
        "  render model {}: {} nodes, {} of the graph's {} matched by name; frame {frame}",
        mode.map_or("-", |m| m.name()),
        skeleton.len(),
        map.matched(),
        graph.nodes.len()
    );
    let mut pose = Pose::rest(&skeleton);
    pose.apply(&clip, frame, &map, 1.0);
    let model = pose.model(&skeleton);
    let rest = skeleton.rest_model();
    println!(
        "  {:3} {:24} {:>34} {:>26} {:>6}  {:>26} {:>8}",
        "", "node", "local rotation xyzw", "local translation", "scale", "model position", "moved"
    );
    for (g, n) in graph.nodes.iter().enumerate() {
        let track = &clip.tracks[g];
        let mark = |has: bool| if has { "*" } else { " " };
        let Some(s) = map.0[g].map(usize::from) else {
            println!("  {g:3} {:24} (not in the render model)", n.name);
            continue;
        };
        let x = pose.local[s];
        let p = model[s].w_axis.truncate();
        let moved = (p - rest[s].w_axis.truncate()).length();
        println!(
            "  {g:3} {:24} {}({:7.3},{:7.3},{:7.3},{:7.3}) {}({:7.3},{:7.3},{:7.3}) {}{:5.2}  ({:7.3},{:7.3},{:7.3}) {:8.3}",
            n.name,
            mark(!track.rotation.is_none()),
            x.rotation.x,
            x.rotation.y,
            x.rotation.z,
            x.rotation.w,
            mark(!track.translation.is_none()),
            x.translation.x,
            x.translation.y,
            x.translation.z,
            mark(!track.scale.is_none()),
            x.scale,
            p.x,
            p.y,
            p.z,
            moved
        );
    }
    println!("  (* = driven by the animation; the rest come from the render model's rest pose{})",
        if a.kind == AnimKind::Overlay { ", overlay added on top" } else { "" });
    Ok(())
}

/// Decodes every animation of a graph; prints failures when `verbose`.
fn check_graph(resources: &blam_cache::Resources<'_>, graph: &Graph, verbose: bool) -> (usize, usize) {
    let (mut ok, mut failed) = (0, 0);
    for i in 0..graph.animations.len() {
        match Clip::load(resources, graph, i) {
            Ok(_) => ok += 1,
            Err(e) => {
                failed += 1;
                if verbose {
                    println!("  #{i} {}: {e}", graph.animations[i].name);
                }
            }
        }
    }
    (ok, failed)
}

fn check_all(cache: &CacheFile) -> Result<()> {
    let resources = cache.resources()?;
    let mut codecs = std::collections::BTreeMap::<(&str, u8), usize>::new();
    let (mut graphs, mut ok, mut failed) = (0, 0, 0);
    let mut reasons = std::collections::BTreeMap::<String, usize>::new();
    for tag in cache.tags.of_class("jmad") {
        let graph = Graph::read(cache, tag)?;
        graphs += 1;
        for i in 0..graph.animations.len() {
            match Clip::load(&resources, &graph, i) {
                Ok(c) => {
                    ok += 1;
                    if let Some(k) = c.static_codec {
                        *codecs.entry(("static", k)).or_default() += 1;
                    }
                    if let Some(k) = c.animated_codec {
                        *codecs.entry(("animated", k)).or_default() += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    let s = e.to_string();
                    let key = s.split_once(": ").map_or(s.clone(), |(_, r)| r.to_string());
                    *reasons.entry(key.chars().take(60).collect()).or_default() += 1;
                }
            }
        }
        resources.clear_page_cache();
    }
    println!("{graphs} graphs, {ok} animations decoded, {failed} not");
    for ((which, k), n) in codecs {
        println!("  {which} codec {k}: {n}");
    }
    for (r, n) in reasons {
        println!("  not decoded ({n}): {r}");
    }
    Ok(())
}
