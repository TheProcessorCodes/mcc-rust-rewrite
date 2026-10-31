//! `blam scripts <map>`: the scenario's HaloScript as readable source
//! (`blam_script`, `docs/format/halo-script.md`).
//!
//! ```text
//! blam scripts <map>                   every global and script
//! blam scripts <map> --grep TEXT       only the scripts whose source holds TEXT
//! blam scripts <map> --functions       the functions called: opcode, name, count
//! blam scripts <map> --summary         counts, script types, the scenario's name tables
//! ```

use anyhow::Result;
use blam_cache::CacheFile;
use blam_script::{Decompiler, ScriptData};

use crate::Args;

pub(crate) fn run(cache: &CacheFile, args: &Args) -> Result<()> {
    let d = ScriptData::read(cache)?;
    if args.has("functions") {
        let counts = d.function_counts();
        for (op, name) in d.function_names() {
            println!("{op:>5} {name:<56} {}", counts.get(&name).copied().unwrap_or(0));
        }
        return Ok(());
    }
    if args.has("summary") {
        let used = d.nodes.iter().filter(|n| !n.is_free()).count();
        println!(
            "{} scripts, {} globals, {} expression slots ({used} used), {} bytes of strings, {} references",
            d.scripts.len(),
            d.globals.len(),
            d.nodes.len(),
            d.strings.len(),
            d.references.len()
        );
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        for s in &d.scripts {
            *kinds.entry(s.kind.name()).or_default() += 1;
        }
        println!("script types: {kinds:?}");
        println!(
            "{} functions used, {} object names, {} trigger volumes, {} cutscene flags, {} point sets, {} folders, {} zone sets",
            d.function_counts().len(),
            d.object_names.len(),
            d.trigger_volumes.len(),
            d.cutscene_flags.len(),
            d.point_sets.len(),
            d.folders.len(),
            d.zone_sets.len()
        );
        return Ok(());
    }
    let grep = args.flag("grep");
    let dec = Decompiler::new(&d);
    if grep.is_none() {
        for g in &d.globals {
            println!("(global {} {} {})", blam_script::type_name(g.value_type), g.name, dec.expression(g.init));
        }
        println!();
    }
    for i in 0..d.scripts.len() {
        let src = dec.script(i);
        if grep.is_none_or(|g| src.contains(g)) {
            println!("{src}\n");
        }
    }
    Ok(())
}
