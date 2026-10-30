//! Halo 2's baked lighting: the structure BSP's lightmap (`ltmp`) gives
//! each cluster and each instance a page of its lightmap bitmap, and the
//! section's vertex buffer 3 gives each vertex a texcoord on that page. The
//! pages are plain A8R8G8B8 light colours on MCC. They are packed here into
//! one atlas per BSP, so the placements can share one texture.
//! Follows `docs/format/halo2.md` section 10.

use blam_texture::{Format, Kind, Texture};

use crate::cache::{Map, Reader};
use crate::{Result, bitmap};

/// One BSP's lightmap pages and which cluster and instance uses which.
pub struct Pages {
    /// The decoded pages, by bitmap index.
    pub pages: Vec<Texture>,
    /// Page of each cluster, by cluster index.
    pub clusters: Vec<Option<u32>>,
    /// Page of each instance, by instance index.
    pub instances: Vec<Option<u32>>,
}

/// Reads the lightmap of the scenario's BSP entry `bsp` (scenario `+0x210`
/// element; the `ltmp` reference at `+0x18`). `None` when it has none.
pub fn read(map: &Map, bsp: Reader<'_>) -> Result<Option<Pages>> {
    let ltmp = bsp.tag_ref(0x18)?;
    let Some(root) = map.tag_data(ltmp) else { return Ok(None) };
    // Lightmap groups (+0x80); the first one carries the BSP's pages.
    let Some(group) = root.block(0x80, 0x84)?.into_iter().next() else { return Ok(None) };
    let bitmap = group.tag_ref(0x18)?;
    if map.tag(bitmap).is_none() {
        return Ok(None);
    }
    let count = bitmap::images(map, bitmap)?.len();
    let mut pages = Vec::with_capacity(count);
    for k in 0..count {
        pages.push(bitmap::decode(map, bitmap, k)?);
    }
    // Render info (4 bytes): the page index i16 at +0 (-1: none), a palette
    // index after it (MCC's pages are not palettised; not read).
    let page_of = |r: Reader<'_>| -> Result<Option<u32>> {
        let p = r.i16(0)?;
        Ok(u32::try_from(p).ok().filter(|&p| (p as usize) < count))
    };
    let clusters = group.block(0x28, 4)?.into_iter().map(page_of).collect::<Result<_>>()?;
    let instances = group.block(0x48, 4)?.into_iter().map(page_of).collect::<Result<_>>()?;
    Ok(Some(Pages { pages, clusters, instances }))
}

/// Pages packed side by side into one A8R8G8B8 texture.
pub struct Atlas {
    pub texture: Texture,
    /// Each page's place: x, y, width, height in texels.
    pub rects: Vec<[u32; 4]>,
}

impl Atlas {
    /// Packs the pages in rows, tallest first, into a power-of-two width.
    /// Pages that are not A8R8G8B8 are left out (their rect is empty).
    pub fn pack(pages: &[Texture], name: &str) -> Atlas {
        let area: u64 = pages.iter().map(|p| p.width as u64 * p.height as u64).sum();
        let widest = pages.iter().map(|p| p.width).max().unwrap_or(1);
        let width = ((area as f64).sqrt().ceil() as u32).max(widest).next_power_of_two();
        let mut order: Vec<usize> = (0..pages.len()).collect();
        order.sort_by_key(|&k| std::cmp::Reverse((pages[k].height, pages[k].width)));
        let mut rects = vec![[0u32; 4]; pages.len()];
        let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
        for k in order {
            let p = &pages[k];
            if p.format != Format::A8R8G8B8 {
                continue;
            }
            if x + p.width > width {
                x = 0;
                y += row;
                row = 0;
            }
            rects[k] = [x, y, p.width, p.height];
            x += p.width;
            row = row.max(p.height);
        }
        let height = (y + row).max(1);
        let mut pixels = vec![0u8; 4 * width as usize * height as usize];
        for (p, r) in pages.iter().zip(&rects) {
            let Some(src) = p.level(0, 0).filter(|_| r[2] > 0) else { continue };
            let row = 4 * r[2] as usize;
            for line in 0..r[3] as usize {
                let at = 4 * ((r[1] as usize + line) * width as usize + r[0] as usize);
                pixels[at..at + row].copy_from_slice(&src[line * row..(line + 1) * row]);
            }
        }
        Atlas {
            texture: Texture {
                name: name.to_string(),
                tag_index: u32::MAX,
                bitmap_index: 0,
                kind: Kind::D2,
                width,
                height,
                layers: 1,
                format: Format::A8R8G8B8,
                srgb: true,
                curve: 0,
                mips: vec![pixels],
            },
            rects,
        }
    }

    /// A page texcoord (0..1 over the page) to the atlas.
    pub fn place(&self, page: u32, uv: [f32; 2]) -> [f32; 2] {
        let r = self.rects[page as usize];
        [
            (r[0] as f32 + uv[0] * r[2] as f32) / self.texture.width as f32,
            (r[1] as f32 + uv[1] * r[3] as f32) / self.texture.height as f32,
        ]
    }

    /// Whether the page made it into the atlas.
    pub fn has(&self, page: u32) -> bool {
        self.rects.get(page as usize).is_some_and(|r| r[2] > 0)
    }
}
