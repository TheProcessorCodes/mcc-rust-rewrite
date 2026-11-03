//! `blam`: look inside a Halo 3 map file.
//!
//! ```text
//! blam info     <map>
//! blam classes  <map>
//! blam tags     <map> [--class CODE] [--grep TEXT]
//! blam strings  <map> [--grep TEXT]
//! blam string-id <map> <hex id>
//! blam dump     <map> <tag index | class:name | name> [--len N] [--skip N]
//! blam geometry <map> [--bsp N[,M]] [--no-objects] [--obj PATH]
//! blam rails    <map> [--bsp N[,M]] [--obj PATH]
//! blam nodes    <map> <mode tag | object tag>      the skeleton and pieces of a render model
//! blam bitmap   <map> <bitm tag> [--index N] [--png PATH]
//! blam shader   <map> <shader tag>
//! blam textures <map> [--png-dir DIR] [--list]  decode every colour texture the map draws
//! blam lightmap <map> [--bsp N] [--samples N]   a BSP's lightmap tag, texcoord links and decoded texels
//! blam movement <map>                           the player's on-foot movement numbers, wu and metres
//! blam weapons  <map> [--all] [--meshes] [--markers]  weapon tags, numbers, markers and placements
//! blam scripts  <map> [--grep TEXT | --functions | --summary]  the scenario's HaloScript as source
//! blam anim     <map> <jmad | object> [--anim NAME|N [--frame F] | --action m:c:t:a | --check]
//!                                               an animation graph, a decoded pose, an action lookup
//! blam anim     <map> --all                     decode every animation of every graph
//! blam ai       <map> [--squads] [--zones] [--characters]  squads, AI zones and characters (`docs/format/ai.md`)
//! ```
//!
//! `<map>` is a path, or a bare name such as `riverworld` looked up in
//! `$HALO3_MAPS`, else the MCC folder chosen in `halo3` setup.

mod anim;
mod ai;
mod cubemaps;
mod geometry;
mod halo2;
mod sound;
mod lightmap;
mod movement;
mod scripts;
mod texture;
mod weapons;

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use blam_cache::{CacheFile, StringId, TagEntry};
use blam_geometry::{Options, Scene, WORLD_UNIT_METRES};
use glam::Vec3;

/// Halo 3's maps folder in the player's MCC (`game_paths`: `$HALO3_MAPS`,
/// the MCC folder chosen in the game's setup).
pub(crate) fn maps_dir() -> PathBuf {
    game_paths::halo3_maps().unwrap_or_default()
}

/// A path, or a bare name: a Halo 3 map in the player's MCC, else a Halo 2
/// (classic) map there (`$HALO2_MAPS` overrides).
/// `h2:NAME` / `h3:NAME` ask for one game when both have a map by that file
/// name (`zanzibar`). Halo 3's Blackout is `lockout.map`, so it goes by
/// `blackout` and a bare `lockout` is Halo 2's Lockout ([`codename`]).
fn resolve_map(arg: &str) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.exists() {
        return p;
    }
    let (game, file) = codename(arg);
    let name = format!("{file}.map");
    let h3 = maps_dir().join(&name);
    let h2 = game_paths::halo2_maps().unwrap_or_default().join(&name);
    match game {
        Some(2) => h2,
        Some(_) => h3,
        None if !h3.exists() && h2.exists() => h2,
        None => h3,
    }
}

/// A map argument to (game, file name without `.map`): `h2:` / `h3:` force
/// the game; `blackout` is Halo 3's `lockout`, and a bare `lockout` is Halo
/// 2's, so the two maps sharing a file name each have a plain name.
fn codename(arg: &str) -> (Option<u8>, &str) {
    let (game, bare) = match (arg.strip_prefix("h2:"), arg.strip_prefix("h3:")) {
        (Some(rest), _) => (Some(2), rest),
        (_, Some(rest)) => (Some(3), rest),
        _ => (None, arg),
    };
    let bare = bare.strip_suffix(".map").unwrap_or(bare);
    match (game, bare) {
        (None | Some(3), "blackout") => (Some(3), "lockout"),
        (None, "lockout") => (Some(2), "lockout"),
        _ => (game, bare),
    }
}

#[cfg(test)]
mod tests {
    use super::codename;

    #[test]
    fn blackout_and_lockout_codenames() {
        assert_eq!(codename("blackout"), (Some(3), "lockout"));
        assert_eq!(codename("lockout"), (Some(2), "lockout"));
        assert_eq!(codename("h3:lockout"), (Some(3), "lockout"));
        assert_eq!(codename("h2:lockout.map"), (Some(2), "lockout"));
        assert_eq!(codename("riverworld"), (None, "riverworld"));
    }
}

pub(crate) struct Args {
    command: String,
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse() -> Option<Args> {
        let mut it = std::env::args().skip(1);
        let command = it.next()?;
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let takes_value = matches!(
                    name,
                    "class"
                        | "grep"
                        | "len"
                        | "skip"
                        | "bsp"
                        | "obj"
                        | "png"
                        | "png-dir"
                        | "index"
                        | "samples"
                        | "wav"
                        | "anim"
                        | "frame"
                        | "action"
                );
                let value = if takes_value { it.next() } else { None };
                flags.push((name.to_string(), value));
            } else {
                positional.push(a);
            }
        }
        Some(Args {
            command,
            positional,
            flags,
        })
    }
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
}

pub(crate) fn usage() -> ! {
    eprintln!(
        "usage:\n  blam info <map>\n  blam classes <map>\n  blam tags <map> [--class CODE] [--grep TEXT]\n  blam strings <map> [--grep TEXT]\n  blam string-id <map> <hex id>\n  blam dump <map> <tag index | class:name | name> [--len N] [--skip N]\n  blam geometry <map> [--bsp N[,M]] [--no-objects] [--obj PATH]\n  blam rails <map> [--obj PATH]\n  blam nodes <map> <mode tag | object tag>\n  blam movement <map>\n  blam weapons <map> [--all] [--meshes] [--markers]\n  blam scripts <map> [--grep TEXT | --functions | --summary]\n  blam anim <map> <jmad | object> [--anim NAME|N [--frame F] | --action m:c:t:a | --check]\n  blam ai <map> [--squads] [--zones] [--characters]\n  blam sound <map> list | show TAG... | wav TAG... [--out DIR] | check | music | dialogue [--grep TEXT]\n\n<map> is a path or a bare name looked up in $HALO3_MAPS, else the MCC folder chosen in halo3 setup."
    );
    std::process::exit(2)
}

fn main() -> Result<()> {
    let Some(args) = Args::parse() else { usage() };
    let Some(map) = args.positional.first() else {
        usage()
    };
    let path = resolve_map(map);
    if blam_h2::is_halo2(&path) {
        return halo2::run(&path, &args);
    }
    let cache = CacheFile::open(&path).with_context(|| format!("opening {}", path.display()))?;
    match args.command.as_str() {
        "info" => info(&cache),
        "classes" => classes(&cache),
        "tags" => tags(&cache, &args),
        "strings" => strings(&cache, &args),
        "string-id" => string_id(&cache, &args),
        "dump" => dump(&cache, &args),
        "blocks" => blocks(&cache, &args),
        "sound" => sound::run(&cache, &args),
        "geometry" => {
            geometry::run(
                &cache,
                bsp_list(&args)?,
                !args.has("no-objects"),
                args.flag("obj").map(Path::new),
            )
        }
        "movement" => movement::run(&cache),
        "scripts" => scripts::run(&cache, &args),
        "rails" => rails(&cache, &args),
        "nodes" => nodes(&cache, &args),
        "weapons" => weapons::run(&cache, &args),
        "anim" => anim::run(&cache, &args),
        "ai" => ai::run(&cache, &args),
        "cubemaps" => cubemaps::run(&cache),
        "bitmap" => {
            let Some(what) = args.positional.get(1) else {
                usage()
            };
            let tag = find_tag_of(&cache, what, "bitm")?;
            let index: u32 = args.flag("index").map_or(Ok(0), str::parse)?;
            texture::bitmap(&cache, tag, index, args.flag("png").map(Path::new))
        }
        "shader" => {
            let Some(what) = args.positional.get(1) else {
                usage()
            };
            let tag = find_tag(&cache, what)?;
            texture::shader(&cache, tag)
        }
        "textures" => texture::textures(&cache, args.flag("png-dir").map(Path::new), args.has("list")),
        "lightmap" => lightmap::run(
            &cache,
            args.flag("bsp").map_or(Ok(0), str::parse)?,
            args.flag("samples").map_or(Ok(6), str::parse)?,
        ),
        _ => usage(),
    }
}

/// `--bsp N[,M]`: only these structure BSPs.
fn bsp_list(args: &Args) -> Result<Option<Vec<u32>>> {
    args.flag("bsp")
        .map(|list| {
            list.split(',')
                .map(|n| n.trim().parse::<u32>().context("--bsp wants numbers"))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()
}

/// `blam rails <map> [--bsp N[,M]] [--obj PATH]`: find the grind rails in a map's collision
/// and print the census and the longest ones; with `--obj`, write them as OBJ
/// polylines in metres, Y up, the frame `geometry --obj` uses.
fn rails(cache: &CacheFile, args: &Args) -> Result<()> {
    let options = Options {
        bsps: bsp_list(args)?,
        objects: false,
        collision: true,
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let mut scene = Scene {
        name: cache.header.map_name.clone(),
        ..Default::default()
    };
    blam_geometry::decode_into(cache, &options, &mut scene)?;
    rails_report(&scene, started, args.flag("obj").map(Path::new))
}

/// The rails of a decoded scene: census, longest, optional OBJ (also used
/// for Halo 2 maps, `halo2.rs`).
pub(crate) fn rails_report(scene: &Scene, started: std::time::Instant, obj: Option<&Path>) -> Result<()> {
    let decoded = started.elapsed().as_secs_f32();
    let (rails, census) = blam_geometry::rails::find_rails(&scene.collision);
    let found = rails.len();
    let rails = blam_geometry::rails::outside_kill_volumes(rails, &scene.trigger_volumes);
    if rails.len() != found {
        eprintln!("kill volumes: {found} rails became {}", rails.len());
    }
    let dry = rails.len();
    let rails = blam_geometry::rails::out_of_deep_water(rails, &scene.water_triangles(), blam_geometry::rails::DEEP_WATER);
    if rails.len() != dry {
        eprintln!("deep water: {dry} rails became {}", rails.len());
    }
    eprintln!(
        "collision decoded in {decoded:.2} s, rails found in {:.2} s",
        started.elapsed().as_secs_f32() - decoded
    );

    let lengths: Vec<f32> = rails
        .iter()
        .map(|r| r.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>())
        .collect();
    let total: f32 = lengths.iter().sum();
    println!(
        "{}: {} collision triangles; {} walkable edges, {} lips, {} runs, {} rails",
        scene.name,
        scene.collision.triangles.len(),
        census.candidates,
        census.lips,
        census.runs,
        census.rails
    );
    println!(
        "  total rail length {:.1} m ({:.1} wu)",
        total * WORLD_UNIT_METRES,
        total
    );
    let mut order: Vec<usize> = (0..rails.len()).collect();
    order.sort_by(|&a, &b| lengths[b].total_cmp(&lengths[a]));
    if !order.is_empty() {
        println!("  longest (length, points, start in wu):");
    }
    for &i in order.iter().take(10) {
        let s = rails[i][0];
        println!(
            "    {:>7.1} m  {:>3} points  [{:.2}, {:.2}, {:.2}]",
            lengths[i] * WORLD_UNIT_METRES,
            rails[i].len(),
            s.x,
            s.y,
            s.z
        );
    }

    if let Some(path) = obj {
        write_rails_obj(&scene.name, &rails, path)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Each rail as one OBJ polyline (`v` lines then one `l` line), vertices in
/// metres with Y up: `(x, y, z) = 3.048 * halo (x, z, -y)`.
fn write_rails_obj(map: &str, rails: &[Vec<Vec3>], path: &Path) -> std::io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    writeln!(
        w,
        "# {map} grind rails from blam_cli; metres, Y up: (x, y, z) = 3.048 * halo (x, z, -y)"
    )?;
    let mut next_vertex = 1u32;
    for (i, rail) in rails.iter().enumerate() {
        writeln!(w, "o rail_{i}")?;
        for p in rail {
            let v = Vec3::new(p.x, p.z, -p.y) * WORLD_UNIT_METRES;
            writeln!(w, "v {} {} {}", v.x, v.y, v.z)?;
        }
        write!(w, "l")?;
        for k in 0..rail.len() as u32 {
            write!(w, " {}", next_vertex + k)?;
        }
        writeln!(w)?;
        next_vertex += rail.len() as u32;
    }
    w.flush()
}

fn info(cache: &CacheFile) -> Result<()> {
    let h = &cache.header;
    println!("file          {}", cache.path().display());
    println!(
        "build         {}{}",
        h.build,
        if cache.is_known_build() {
            ""
        } else {
            "  (not a build this tool was written against)"
        }
    );
    println!("map name      {}", h.map_name);
    println!("scenario      {}", h.scenario_path);
    println!(
        "file size     {:#x} ({} MiB)",
        h.file_size,
        h.file_size / (1 << 20)
    );
    println!(
        "tags          {}  classes {}",
        h.tag_count,
        cache.tags.classes.len()
    );
    println!(
        "strings       {}  namespaces {}",
        h.string_count, h.string_namespace_count
    );
    println!(
        "virtual base  {:#x}  tag index {:#x}",
        h.virtual_base, h.tag_index_pointer
    );
    println!("sections      (virtual address, size -> file offset)");
    for (i, name) in ["debug", "resource", "tag", "locale"].iter().enumerate() {
        let s = h.sections[i];
        println!(
            "  {i} {name:<9} {:#010x} {:#010x} -> {:#010x}",
            s.address,
            s.size,
            h.section_file_base(i)
        );
    }
    println!("partitions    (virtual address, size)");
    for (i, p) in h.partitions.iter().enumerate() {
        println!("  {i} {:#012x} {:#010x}", p.address, p.size);
    }
    for code in ["scnr", "matg", "play", "zone", "ugh!"] {
        let tag = if code == "scnr" {
            cache.scenario()
        } else {
            cache.global_tag(code)
        };
        match tag {
            Some(t) => println!(
                "{code}          #{:<5} {}  data at {:#x}",
                t.index,
                t.name(),
                cache.tag_offset(t).map(|o| o as u64).unwrap_or(0)
            ),
            None => println!("{code}          (none)"),
        }
    }
    Ok(())
}

fn classes(cache: &CacheFile) -> Result<()> {
    let mut counts = vec![0usize; cache.tags.classes.len()];
    for t in cache.tags.iter() {
        counts[t.class_index as usize] += 1;
    }
    for (i, c) in cache.tags.classes.iter().enumerate() {
        println!(
            "{i:>3} {} {} {} {:>5}  {}",
            c.code,
            c.parent,
            c.grandparent,
            counts[i],
            cache.class_name(c)
        );
    }
    Ok(())
}

fn tags(cache: &CacheFile, args: &Args) -> Result<()> {
    let class = args.flag("class");
    let grep = args.flag("grep").map(|g| g.to_ascii_lowercase());
    let mut n = 0;
    for t in cache.tags.iter() {
        let c = cache.tags.class_of(t);
        if class.is_some_and(|c2| c.code != c2) {
            continue;
        }
        if grep
            .as_deref()
            .is_some_and(|g| !t.name().to_ascii_lowercase().contains(g))
        {
            continue;
        }
        println!(
            "{:>5} {:#06x} {} {:#010x} {}",
            t.index,
            t.salt,
            c.code,
            cache.tag_offset(t).unwrap_or(0),
            t.name()
        );
        n += 1;
    }
    eprintln!("{n} tags");
    Ok(())
}

fn strings(cache: &CacheFile, args: &Args) -> Result<()> {
    let grep = args.flag("grep").map(|g| g.to_ascii_lowercase());
    for (i, s) in cache.strings.iter() {
        if grep
            .as_deref()
            .is_some_and(|g| !s.to_ascii_lowercase().contains(g))
        {
            continue;
        }
        println!("{i:>6} {s}");
    }
    Ok(())
}

fn string_id(cache: &CacheFile, args: &Args) -> Result<()> {
    let Some(hex) = args.positional.get(1) else {
        usage()
    };
    let id =
        u32::from_str_radix(hex.trim_start_matches("0x"), 16).context("string id must be hex")?;
    let id = StringId(id);
    let l = cache.strings.layout;
    println!(
        "id {:#010x}: namespace {} index {} length {} -> table position {} = {:?}",
        id.0,
        l.namespace(id),
        l.index(id),
        l.length(id),
        cache.strings.index_of(id),
        cache.string(id)
    );
    Ok(())
}

pub(crate) fn find_tag<'a>(cache: &'a CacheFile, what: &str) -> Result<&'a TagEntry> {
    if let Ok(i) = what.parse::<u32>() {
        return cache.tags.get(i).with_context(|| format!("no tag #{i}"));
    }
    if let Some((class, name)) = what.split_once(':') {
        return cache
            .tags
            .find(class, name)
            .with_context(|| format!("no {class} tag named {name}"));
    }
    let mut matches = cache
        .tags
        .iter()
        .filter(|t| t.name().eq_ignore_ascii_case(what));
    let first = matches
        .next()
        .with_context(|| format!("no tag named {what}"))?;
    if matches.next().is_some() {
        bail!("several tags are named {what}; say class:name");
    }
    Ok(first)
}

/// Like [`find_tag`], but a bare name is looked up in one class first, so
/// `blam bitmap riverworld <name>` works when a shader shares the name.
fn find_tag_of<'a>(cache: &'a CacheFile, what: &str, class: &str) -> Result<&'a TagEntry> {
    if what.parse::<u32>().is_err()
        && !what.contains(':')
        && let Some(t) = cache.tags.find(class, what)
    {
        return Ok(t);
    }
    find_tag(cache, what)
}

/// `blam blocks <map> <tag> [--len N] [--skip N]`: every plausible 12-byte tag
/// block header in the first N bytes of a tag (from `--skip`), with the
/// distance to the next block's elements divided by the count (an upper bound
/// on the element size). For finding layouts by hand.
fn blocks(cache: &CacheFile, args: &Args) -> Result<()> {
    let Some(what) = args.positional.get(1) else {
        usage()
    };
    let tag = find_tag(cache, what)?;
    let len: usize = args.flag("len").map_or(Ok(0x400), str::parse)?;
    let skip: usize = args.flag("skip").map_or(Ok(0), str::parse)?;
    let start = cache.tag_offset(tag)? + skip;
    let file_len = cache.bytes().len();
    let mut found = Vec::new();
    for off in (0..len).step_by(4) {
        let mut r = cache.reader_at(start + off);
        let (Ok(count), Ok(Some(at))) = (r.u32(), r.pointer()) else {
            continue;
        };
        if (1..200_000).contains(&count) && at < file_len {
            found.push((off, count, at));
        }
    }
    let mut starts: Vec<usize> = found.iter().map(|f| f.2).collect();
    starts.sort_unstable();
    println!(
        "#{} {} {}  tag data at {:#x}",
        tag.index,
        cache.tags.class_of(tag).code,
        tag.name(),
        start - skip
    );
    for (off, count, at) in found {
        let next = starts.iter().find(|&&s| s > at).copied().unwrap_or(at);
        println!(
            "+{:#06x}  count {count:7}  at {at:#x}  gap/count {:9.1}",
            off + skip,
            (next - at) as f64 / count as f64
        );
    }
    Ok(())
}

fn dump(cache: &CacheFile, args: &Args) -> Result<()> {
    let Some(what) = args.positional.get(1) else {
        usage()
    };
    let tag = find_tag(cache, what)?;
    let len: usize = args.flag("len").map_or(Ok(256), str::parse)?;
    let skip: usize = args.flag("skip").map_or(Ok(0), str::parse)?;
    let start = cache.tag_offset(tag)? + skip;
    let bytes = cache.bytes();
    let end = (start + len).min(bytes.len());
    println!(
        "#{} {} {}  tag data at {:#x} (+{skip:#x})",
        tag.index,
        cache.tags.class_of(tag).code,
        tag.name(),
        start - skip
    );
    for row in (start..end).step_by(16) {
        let chunk = &bytes[row..(row + 16).min(end)];
        print!("{:#010x} +{:#06x} ", row, row - start + skip);
        for (i, b) in chunk.iter().enumerate() {
            if i % 4 == 0 {
                print!(" ");
            }
            print!("{b:02x}");
        }
        for i in chunk.len()..16 {
            if i % 4 == 0 {
                print!(" ");
            }
            print!("  ");
        }
        print!("  ");
        for &b in chunk {
            print!(
                "{}",
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            );
        }
        println!();
    }
    if args.has("verbose") {
        println!("salt {:#06x} class index {}", tag.salt, tag.class_index);
    }
    Ok(())
}

/// `blam nodes <map> <mode tag | object tag>`: a render model's skeleton as a
/// tree, and its pieces with their bone or skin.
fn nodes(cache: &CacheFile, args: &Args) -> Result<()> {
    let Some(what) = args.positional.get(1) else {
        usage()
    };
    let tag = find_tag(cache, what)?;
    let mode = if cache.tags.class_of(tag).code == "mode" {
        tag
    } else {
        blam_geometry::scenario::render_model_of(cache, tag)?
            .with_context(|| format!("{} has no render model", tag.name()))?
    };
    let resources = cache.resources()?;
    let mut materials = blam_geometry::materials::MaterialTable::default();
    let mut scene = blam_geometry::Scene::default();
    let parts =
        blam_geometry::render_model::decode(cache, &resources, &mut materials, mode, &mut scene)?;
    println!(
        "{} (#{} {}): {} bones, {} pieces, {} triangles",
        parts.name,
        mode.index,
        mode.name(),
        parts.bones.len(),
        parts.pieces.len(),
        scene.triangle_count_of_meshes()
    );

    // Bones as a tree.
    let n = parts.bones.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (i, b) in parts.bones.iter().enumerate() {
        match b.parent.map(usize::from).filter(|&p| p < n) {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    fn walk(
        parts: &blam_geometry::render_model::ModelParts,
        children: &[Vec<usize>],
        i: usize,
        depth: usize,
    ) {
        let b = &parts.bones[i];
        let w = b.world.w_axis;
        println!(
            "{:>3} {}{}  local [{:.3}, {:.3}, {:.3}]  model [{:.3}, {:.3}, {:.3}]",
            i,
            "  ".repeat(depth),
            b.name,
            b.position.x,
            b.position.y,
            b.position.z,
            w.x,
            w.y,
            w.z
        );
        for &c in &children[i] {
            walk(parts, children, c, depth + 1);
        }
    }
    for r in roots {
        walk(&parts, &children, r, 0);
    }

    println!("pieces:");
    for p in &parts.pieces {
        let mesh = &scene.meshes[p.mesh as usize];
        let how = match (&mesh.skin, p.bone) {
            (Some(skin), _) => {
                let max_joint = skin.joints.iter().flatten().copied().max().unwrap_or(0);
                format!("skinned, joints up to {max_joint}")
            }
            (None, Some(b)) => format!(
                "rigid on bone {b} ({})",
                parts.bones.get(b as usize).map_or("?", |x| x.name.as_str())
            ),
            (None, None) => "rigid, no bone".to_string(),
        };
        println!(
            "  {:<40} {:>7} triangles  {how}",
            mesh.name,
            mesh.triangle_count()
        );
    }
    Ok(())
}
