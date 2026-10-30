//! Paints the stock THQ / Green sheet Cortana with the 450F from the
//! user's Reflex install. Needs `REFLEX_STOCK_KITS`, a folder holding the
//! stock `HR_MX_GraphicKits` and `MX_GraphicKits` pairs (Reflex's own
//! `Database` folder works while it still has the stock kit); skipped
//! without it or without Reflex. The sheets are written as BMPs under
//! cargo's test temp folder to look at.

use std::path::{Path, PathBuf};

use reflex_assets::bike::{self, KIT};
use reflex_assets::package::Package;
use reflex_assets::texture::{self, Rgba};
use reflex_assets::{cortana, find_reflex};

fn write_bmp(path: &Path, t: &Rgba) {
    let (w, h) = (t.width as usize, t.height as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    for v in [size as u32, 0, 54, 40, w as u32, h as u32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    for v in [0u32, (row * h) as u32, 2835, 2835, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for y in (0..h).rev() {
        let start = b.len();
        for p in &t.pixels[y * w..(y + 1) * w] {
            b.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        b.resize(start + row, 0);
    }
    std::fs::write(path, b).expect("bmp");
}

#[test]
fn stock_kit_paints_cortana() {
    let (Some(stock), Some(root)) = (std::env::var_os("REFLEX_STOCK_KITS").map(PathBuf::from), find_reflex()) else {
        eprintln!("REFLEX_STOCK_KITS or Reflex missing; skipped");
        return;
    };
    let model = bike::load(&root).expect("450F");
    let pieces: Vec<&bike::Part> = model.parts.iter().filter(|p| p.texture.as_deref() == Some(KIT)).collect();
    assert!(!pieces.is_empty());
    for (stem, sub, name) in [
        ("HR_MX_GraphicKits", "HR_MX_NeptuneF_THQ_Green", "HR_MX_Plstc_Neptune_Green_THQ"),
        ("MX_GraphicKits", "MX_NeptuneF_THQ_Green", "MX_Plstc_Neptune_Green_THQ"),
    ] {
        let kits = Package::open(&stock, stem).expect("stock kits");
        let sheet = texture::read(&kits.get(sub, name, "texture").expect("sheet")).expect("decode");
        assert!(!cortana::is_cortana(&sheet), "{stem}: the sheet given is not the stock green one");
        let started = std::time::Instant::now();
        let painted = cortana::paint(&sheet, &pieces, |b| model.rest(b));
        eprintln!("{stem}: {}x{} painted in {:.2} s", sheet.width, sheet.height, started.elapsed().as_secs_f32());
        assert!(cortana::is_cortana(&painted), "{stem}: not blue enough");
        let kept = cortana::kept(&sheet);
        let n = kept.iter().filter(|&&k| k).count();
        assert!(n > (sheet.pixels.len() / 100), "{stem}: only {n} plate texels");
        for (k, (a, b)) in sheet.pixels.iter().zip(&painted.pixels).enumerate() {
            if kept[k] {
                assert_eq!(a[..3], b[..3], "{stem}: plate texel {k} changed");
            }
        }
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cortana_{stem}.bmp"));
        write_bmp(&out, &painted);
        eprintln!("{stem}: {} plate/hardware texels kept; written to {}", n, out.display());
    }
}
