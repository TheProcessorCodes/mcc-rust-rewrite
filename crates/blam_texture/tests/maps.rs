//! Checks against the user's own riverworld.map (Valhalla), pinning the
//! values recorded in `docs/format/bitmap.md` section 8 and
//! `docs/format/shaders.md` section 8. Skips quietly when the map is not
//! installed.

use std::path::PathBuf;

use blam_cache::CacheFile;
use blam_texture::bitmap::{self, Kind};
use blam_texture::{Blend, Format, rgba, shader};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps",
            )
        });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

#[test]
fn riverworld_bitmaps() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(path).unwrap();
    let resources = cache.resources().unwrap();

    // bitmap.md 11.2: mountains_snow, tag 112.
    let tag = bitmap::read_tag(&cache, 112).unwrap();
    assert_eq!(tag.name, r"levels\multi\riverworld\sky\bitmaps\mountains_snow");
    let h = tag.bitmaps[0];
    assert_eq!((h.width, h.height, h.depth, h.flags, h.kind, h.format, h.mipmap_count, h.curve), (1024, 512, 1, 0x20, 0, 14, 10, 3));
    assert_eq!(tag.resources[0].0, 0xE178_0015);
    let loc = bitmap::locate(&resources, &tag, 0).unwrap();
    assert_eq!(loc.entry, 21);
    assert_eq!(loc.segment, 14);
    assert_eq!(loc.definition.primary_size, 0x15568);
    assert_eq!(loc.definition.secondary_size, 0x40000);
    assert_eq!(loc.definition.levels, 11);
    assert_eq!(loc.definition.dxgi, 71);
    assert_eq!((loc.primary.page, loc.primary.offset), (1420, 0xE0000));
    let sec = loc.secondary.as_ref().unwrap();
    assert_eq!((sec.page, sec.offset, sec.page_decompressed), (1424, 0, 0x40000));

    // 11.3: the first bytes of mip 0.
    let t = bitmap::decode_from(&resources, &tag, 0).unwrap();
    assert_eq!((t.format, t.srgb, t.kind, t.layers), (Format::Bc1, false, Kind::D2, 1));
    assert_eq!(t.mips.len(), 11);
    assert_eq!(&t.mips[0][..8], &[0x55, 0x84, 0x4e, 0x63, 0x2d, 0xa9, 0x2b, 0x2b]);
    assert_eq!(t.mips[0].len(), 0x40000);
    assert_eq!(t.mips[10].len(), 8);

    let floor = bitmap::decode(&cache, &resources, 11437, 0).unwrap();
    assert_eq!(floor.name, r"levels\multi\riverworld\bitmaps\forerunner\cust_base_floor_dif");
    assert_eq!(floor.format, Format::Bc3);
    assert_eq!(&floor.mips[0][..8], &[0x00, 0x1b, 0x49, 0x42, 0x86, 0xdb, 0x24, 0xc0]);

    let bump = bitmap::decode(&cache, &resources, 11438, 0).unwrap();
    assert_eq!(bump.format, Format::Bc5);

    let gray = bitmap::decode(&cache, &resources, 18, 0).unwrap();
    assert_eq!(gray.format, Format::A8R8G8B8);
    assert_eq!(gray.mips, vec![vec![0x40, 0x40, 0x40, 0xff]]);

    let alpha = bitmap::decode(&cache, &resources, 53, 0).unwrap();
    assert_eq!((alpha.format, alpha.width, alpha.mips.len()), (Format::A8, 64, 7));
    assert_eq!(&alpha.mips[0][..8], &[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);

    let man_cannon = bitmap::decode(&cache, &resources, 6631, 0).unwrap();
    assert!(man_cannon.srgb, "xRGB curve is sampled as sRGB (DXGI 78)");

    // 11.5: the cube map, six faces of three levels in one page.
    let cube = bitmap::decode(&cache, &resources, 6882, 0).unwrap();
    assert_eq!((cube.kind, cube.layers, cube.mips.len()), (Kind::Cube, 6, 3));
    assert_eq!(cube.byte_len(), 0x1F800);
    assert_eq!(cube.level(2, 5).unwrap().len(), 0x400);

    // 11.1 and 11.7: every bitmap of every tag either decodes or reports an
    // error (17 dead references in riverworld); nothing panics.
    let (mut tags, mut blocks, mut decoded, mut failed) = (0, 0, 0, 0);
    for t in cache.tags.of_class("bitm") {
        let tag = bitmap::read_tag(&cache, t.index).unwrap();
        tags += 1;
        assert_eq!(tag.bitmaps.len(), tag.resources.len(), "{}", tag.name);
        assert_eq!(tag.interleaved_count, 0);
        blocks += tag.bitmaps.len();
        // Decoding all 2464 inflates every page; do the first of each tag.
        match bitmap::decode_from(&resources, &tag, 0) {
            Ok(_) => decoded += 1,
            // Only dead references may fail; a layout error is a bug.
            Err(blam_texture::Error::NoData(_)) => failed += 1,
            Err(e) => panic!("{}: {e}", tag.name),
        }
    }
    eprintln!("{tags} bitm tags, {blocks} bitmaps; first bitmap of each: {decoded} decoded, {failed} without pixels");
    assert_eq!((tags, blocks), (2204, 2464));
    assert!(failed <= 17, "{failed} bitmaps failed to decode");
    assert!(decoded >= 2204 - 17);
}

#[test]
fn riverworld_shaders() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(path).unwrap();

    // shaders.md 8.1
    let rock = shader::resolve(&cache, 11454).unwrap();
    assert_eq!(rock.class, "rmsh");
    assert_eq!(rock.options.len(), 13);
    assert_eq!(rock.option("specular_mask"), Some("specular_mask_from_diffuse"));
    let base = rock.base.as_ref().unwrap();
    assert_eq!(base.name, r"levels\multi\riverworld\bitmaps\cliff_rock_base_brown");
    assert_eq!(base.bitmap, 11455);
    assert_eq!(base.xform.scale, [22.0, 22.0]);
    assert_eq!(rock.map("detail_map").unwrap().xform.scale, [122.0, 122.0]);
    assert_eq!(rock.blend, Blend::Opaque);
    assert!(!rock.alpha_test && !rock.two_sided && !rock.special);

    // 8.2: terrain layers and their tiling.
    let ground = shader::resolve(&cache, 11594).unwrap();
    let terrain = ground.terrain.as_ref().unwrap();
    assert_eq!(terrain.blend_map.as_ref().unwrap().name, r"levels\multi\riverworld\bitmaps\cliff_snow_distant_blend");
    let scales: Vec<[f32; 2]> = terrain.layers.iter().map(|l| l.as_ref().unwrap().base.xform.scale).collect();
    assert_eq!(scales, vec![[40.0, 80.0], [6.0, 12.0], [1.0, 1.0], [15.0, 30.0]]);
    assert_eq!(terrain.layers[3].as_ref().unwrap().base.name, r"levels\multi\riverworld\bitmaps\cliff_rock_base");

    // HaloExport's mean blend weights for this blend map.
    let resources = cache.resources().unwrap();
    let blend = bitmap::decode(&cache, &resources, terrain.blend_map.as_ref().unwrap().bitmap, 0).unwrap();
    let px = rgba::decode_level(blend.format, blend.width, blend.height, blend.level(0, 0).unwrap()).unwrap();
    let w = rgba::channel_means(&px);
    for (got, want) in w.iter().zip([0.3583, 0.1398, 0.1904, 0.3486]) {
        assert!((got - want).abs() < 0.003, "{w:?}");
    }

    // 8.3: foliage is alpha-tested and two-sided.
    let leaf = shader::resolve(&cache, 11652).unwrap();
    assert!(leaf.alpha_test && leaf.two_sided);
    assert_eq!(leaf.base.as_ref().unwrap().name, r"levels\solo\100_citadel\bitmaps\cit_tree_leafa");

    // 8.4: water draws its watercolor texture, tinted by water_diffuse.
    let water = shader::resolve(&cache, 11609).unwrap();
    assert_eq!(water.base.as_ref().unwrap().name, r"levels\multi\riverworld\bitmaps\riverworld_water_diffuse");
    assert!(water.is_translucent());
    assert_eq!(water.map("foam_texture").unwrap().xform.scale, [2.0, 1.0]);

    // 8.5: glass is pre-multiplied alpha.
    let glass = shader::resolve(&cache, 11504).unwrap();
    assert_eq!(glass.blend, Blend::PreMultiplied);
    assert_eq!(glass.map("specular_mask_texture").unwrap().xform.scale, [2.5, 2.5]);

    // 8.6: constant-colour shaders have no base map but a colour.
    let null_light = shader::resolve(&cache, 11556).unwrap();
    assert!(null_light.base.is_none());
    assert!(null_light.albedo_color.is_some());

    // Every render method in the map resolves.
    let mut n = 0;
    for t in cache.tags.iter() {
        let class = cache.tags.class_of(t).code.as_str();
        if matches!(class, "rmsh" | "rmtr" | "rmfl" | "rmw " | "rmd " | "rmhg") {
            shader::resolve(&cache, t.index).unwrap_or_else(|e| panic!("{}: {e}", t.name()));
            n += 1;
        }
    }
    assert_eq!(n, 988);
}

/// Texture V runs down the image, as in D3D and wgpu: on the sky's mountain
/// backdrop (`mountains_snow`, snow peaks at the top of the image) the
/// higher a vertex is, the smaller its V. So the viewer must not flip V.
#[test]
fn riverworld_v_runs_down() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(path).unwrap();
    let mut scene = blam_geometry::Scene::default();
    let options = blam_geometry::Options {
        collision: false,
        ..Default::default()
    };
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let mountain_materials: Vec<u32> = scene
        .materials
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.shader_tag
                .and_then(|t| shader::resolve(&cache, t).ok())
                .and_then(|s| s.base)
                .is_some_and(|b| b.bitmap == 112)
        })
        .map(|(i, _)| i as u32)
        .collect();
    assert!(!mountain_materials.is_empty(), "no material draws mountains_snow");

    // Correlation between height and V over the vertices drawn with it.
    let (mut n, mut sz, mut sv, mut szz, mut svv, mut szv) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for p in &scene.placements {
        let mesh = &scene.meshes[p.mesh as usize];
        for part in mesh.parts.iter().filter(|part| mountain_materials.contains(&part.material)) {
            for &i in &part.indices {
                let z = p.transform.transform_point3(mesh.positions[i as usize]).z as f64;
                let v = mesh.uvs[i as usize][1] as f64;
                n += 1.0;
                sz += z;
                sv += v;
                szz += z * z;
                svv += v * v;
                szv += z * v;
            }
        }
    }
    assert!(n > 0.0);
    let cov = szv / n - (sz / n) * (sv / n);
    let r = cov / ((szz / n - (sz / n).powi(2)).sqrt() * (svv / n - (sv / n).powi(2)).sqrt());
    eprintln!("mountains_snow: height/V correlation {r:.3} over {n} vertex uses");
    assert!(r < -0.5, "height/V correlation {r:.3}: V should fall as height rises");
}
