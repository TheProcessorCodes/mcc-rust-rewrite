//! Checks against the user's own Reflex install; skipped when there is none.

use reflex_assets::{bike, find_reflex};

#[test]
fn the_450f_loads() {
    let Some(root) = find_reflex() else {
        eprintln!("no Reflex install; skipped");
        return;
    };
    let m = bike::load(&root).expect("450F");
    let names: Vec<&str> = m.bones.iter().map(|b| b.name.as_str()).collect();
    eprintln!("kit: {}; bones {names:?}", m.kit);
    for (i, b) in m.bones.iter().enumerate() {
        let r = m.rest(i);
        eprintln!("{i:2} {:<20} parent {:?} at [{:.3}, {:.3}, {:.3}] x {:.3?} y {:.3?} z {:.3?}", b.name, b.parent, r[3][0], r[3][1], r[3][2], &r[0][..3], &r[1][..3], &r[2][..3]);
    }
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &m.parts {
        let rest = m.rest(p.bone);
        let (mut plo, mut phi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &v in &p.positions {
            let w = bike::apply(&rest, v);
            for c in 0..3 {
                plo[c] = plo[c].min(w[c]);
                phi[c] = phi[c].max(w[c]);
            }
        }
        for c in 0..3 {
            lo[c] = lo[c].min(plo[c]);
            hi[c] = hi[c].max(phi[c]);
        }
        eprintln!(
            "{:<24} bone {:2} {:5} tris {:?} cut {} [{:.2} {:.2} {:.2}]..[{:.2} {:.2} {:.2}]",
            p.slot, p.bone, p.indices.len() / 3, p.texture, p.cutout, plo[0], plo[1], plo[2], phi[0], phi[1], phi[2]
        );
    }
    eprintln!("bounds [{:.3} {:.3} {:.3}]..[{:.3} {:.3} {:.3}]", lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]);
    for (k, t) in &m.textures {
        eprintln!("texture {k}: {}x{}", t.width, t.height);
    }
    assert!(m.bone("OBJ_FRT_Tire").is_some() && m.bone("OBJ_BCK_Tire").is_some());
    assert!(m.parts.len() > 20);
    assert!(reflex_assets::cortana::is_cortana(&m.textures[bike::KIT]));
}
