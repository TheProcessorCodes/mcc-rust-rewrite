//! Getting a map's scripts ready: which objects they move (drawn on their
//! own), the object table, the folders.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use blam_cache::CacheFile;
use blam_geometry::{Kind, Scene};
use blam_script::{NodeKind, ScriptData, ValueType};

use crate::objects::{ObjectInfo, Objects, Section};
use crate::vm::{Config, Runtime};

/// The object names and folders any script constant names.
pub fn named_by_scripts(data: &ScriptData) -> (HashSet<u16>, HashSet<u16>) {
    let mut names = HashSet::new();
    let mut folders = HashSet::new();
    for n in &data.nodes {
        if n.is_free() || n.kind() != NodeKind::Constant {
            continue;
        }
        let i = n.as_i16();
        if i < 0 {
            continue;
        }
        if n.value_type.names_object() {
            names.insert(i as u16);
        } else if n.value_type == ValueType::FOLDER {
            folders.insert(i as u16);
        }
    }
    (names, folders)
}

/// Marks the scene objects scripts can move or hide as dynamic, so the
/// world draws each on its own. Returns how many.
pub fn mark_dynamic(scene: &mut Scene, data: &ScriptData) -> usize {
    let (names, folders) = named_by_scripts(data);
    let mut n = 0;
    for o in &mut scene.objects {
        o.dynamic = o.name_index.is_some_and(|i| names.contains(&i))
            || o.folder.is_some_and(|f| folders.contains(&f))
            || o.starts_absent();
        n += o.dynamic as usize;
    }
    n
}

/// Builds the runtime for a decoded map.
pub fn runtime(cache: &CacheFile, scene: &Scene, data: Arc<ScriptData>, config: Config) -> Runtime {
    let mut sections: HashMap<u32, Vec<Section>> = HashMap::new();
    let mut info = Vec::with_capacity(scene.objects.len());
    let mut by_name: Vec<Option<u32>> = vec![None; data.object_names.len()];
    for (i, o) in scene.objects.iter().enumerate() {
        let secs = if o.dynamic {
            sections
                .entry(o.tag)
                .or_insert_with(|| {
                    let Some(tag) = cache.tags.get(o.tag) else {
                        return Vec::new();
                    };
                    blam_geometry::objects::damage_sections(cache, tag)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| Section {
                            effect: s
                                .responses
                                .iter()
                                .find_map(|r| r.effect.or(r.effect2))
                                .and_then(|t| cache.tags.get(t))
                                .map(|t| t.name().to_string()),
                            name: s.name,
                        })
                        .collect()
                })
                .clone()
        } else {
            Vec::new()
        };
        if let Some(n) = o.name_index
            && let Some(slot) = by_name.get_mut(n as usize)
        {
            *slot = Some(i as u32);
        }
        info.push(ObjectInfo {
            name: o.name.clone(),
            scene: Some(i as u32),
            object_type: o
                .name_index
                .and_then(|n| data.object_names.get(n as usize))
                .map_or(kind_type(o.kind), |n| n.object_type),
            base: o.transform,
            folder: o.folder,
            starts_absent: o.starts_absent(),
            sections: secs,
            tag_path: o.tag_path.clone(),
            tag: Some(o.tag),
        });
    }
    // Named objects nothing draws (sound scenery, bipeds, vehicles).
    for (n, name) in data.object_names.iter().enumerate() {
        if by_name[n].is_none() {
            by_name[n] = Some(info.len() as u32);
            info.push(ObjectInfo {
                name: Some(name.name.clone()),
                scene: None,
                object_type: name.object_type,
                base: glam::Mat4::IDENTITY,
                folder: None,
                starts_absent: false,
                sections: Vec::new(),
                tag_path: String::new(),
                tag: None,
            });
        }
    }
    let mut folders: Vec<Vec<u32>> = vec![Vec::new(); data.folders.len()];
    for (i, o) in info.iter().enumerate() {
        if let Some(f) = o.folder
            && let Some(list) = folders.get_mut(f as usize)
        {
            list.push(i as u32);
        }
    }
    let objects = Objects::new(info, by_name);
    Runtime::new(data, objects, scene.trigger_volumes.clone(), folders, config)
}

fn kind_type(kind: Kind) -> i16 {
    match kind {
        Kind::Vehicle => 1,
        Kind::Machine => 7,
        Kind::Control => 8,
        Kind::Crate => 10,
        _ => 6,
    }
}
