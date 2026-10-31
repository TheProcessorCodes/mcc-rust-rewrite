//! A flat grid of columns over triangles, for short rays and small spheres.
//!
//! Each triangle goes into every column its footprint covers, at one of
//! three levels so a few huge ones cannot fill the grid: small triangles
//! into fine cells, larger ones into cells [`COARSE`] times wider, and the
//! few that would still cover more than [`MAX_COARSE_CELLS`] (Forge World's
//! outer box, about 9000 x 9000 world units) into one list every query
//! returns. Before this, those boxes alone put 235 million entries into the
//! bike's grid and 100 million into the rail finder's.

use std::collections::HashMap;

/// A coarse cell is this many fine cells wide.
const COARSE: i32 = 16;
/// A triangle covering more fine cells than this goes to the coarse level.
const MAX_FINE_CELLS: i64 = 64;
/// ... and more coarse cells than this to the list every query sees.
const MAX_COARSE_CELLS: i64 = 1024;

#[derive(Clone, Debug, Default)]
pub struct ColumnGrid {
    cell: f32,
    fine: HashMap<(i32, i32), Vec<u32>>,
    coarse: HashMap<(i32, i32), Vec<u32>>,
    everywhere: Vec<u32>,
}

impl ColumnGrid {
    /// An empty grid of `cell`-wide fine columns.
    pub fn new(cell: f32) -> Self {
        Self {
            cell,
            ..Self::default()
        }
    }

    pub fn cell(&self) -> f32 {
        self.cell
    }

    fn range(&self, lo: f32, hi: f32, size: f32) -> (i32, i32) {
        ((lo / size).floor() as i32, (hi / size).floor() as i32)
    }

    /// Files item `index` under the columns its footprint `lo..=hi` (the two
    /// grid axes) covers. Footprints that are not finite are dropped.
    pub fn insert(&mut self, index: u32, lo: [f32; 2], hi: [f32; 2]) {
        if !(lo[0].is_finite() && lo[1].is_finite() && hi[0].is_finite() && hi[1].is_finite()) {
            return;
        }
        let span = |(a, b): (i32, i32)| (b as i64 - a as i64 + 1).max(1);
        let (fx, fy) = (self.range(lo[0], hi[0], self.cell), self.range(lo[1], hi[1], self.cell));
        if span(fx) * span(fy) <= MAX_FINE_CELLS {
            for x in fx.0..=fx.1 {
                for y in fy.0..=fy.1 {
                    self.fine.entry((x, y)).or_default().push(index);
                }
            }
            return;
        }
        let size = self.cell * COARSE as f32;
        let (cx, cy) = (self.range(lo[0], hi[0], size), self.range(lo[1], hi[1], size));
        if span(cx) * span(cy) <= MAX_COARSE_CELLS {
            for x in cx.0..=cx.1 {
                for y in cy.0..=cy.1 {
                    self.coarse.entry((x, y)).or_default().push(index);
                }
            }
            return;
        }
        self.everywhere.push(index);
    }

    /// Calls `f` with every item filed under a column that `lo..=hi` touches.
    /// An item can come more than once; callers that care deduplicate.
    pub fn query(&self, lo: [f32; 2], hi: [f32; 2], mut f: impl FnMut(u32)) {
        let mut visit = |map: &HashMap<(i32, i32), Vec<u32>>, size: f32| {
            let (x0, x1) = self.range(lo[0], hi[0], size);
            let (y0, y1) = self.range(lo[1], hi[1], size);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if let Some(list) = map.get(&(x, y)) {
                        list.iter().for_each(|&i| f(i));
                    }
                }
            }
        };
        visit(&self.fine, self.cell);
        if !self.coarse.is_empty() {
            visit(&self.coarse, self.cell * COARSE as f32);
        }
        self.everywhere.iter().for_each(|&i| f(i));
    }

    /// Entries filed, all levels together.
    pub fn entries(&self) -> usize {
        self.fine.values().chain(self.coarse.values()).map(Vec::len).sum::<usize>() + self.everywhere.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(g: &ColumnGrid, lo: [f32; 2], hi: [f32; 2]) -> Vec<u32> {
        let mut v = Vec::new();
        g.query(lo, hi, |i| v.push(i));
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn every_level_is_found_where_it_lies() {
        let mut g = ColumnGrid::new(1.0);
        g.insert(0, [0.2, 0.2], [0.8, 0.8]); // fine
        g.insert(1, [0.0, 0.0], [40.0, 40.0]); // coarse
        g.insert(2, [-5000.0, -5000.0], [5000.0, 5000.0]); // everywhere
        assert_eq!(found(&g, [0.5, 0.5], [0.5, 0.5]), [0, 1, 2]);
        assert_eq!(found(&g, [30.0, 30.0], [30.5, 30.5]), [1, 2]);
        assert_eq!(found(&g, [100.0, 100.0], [100.0, 100.0]), [2]);
        assert!(g.entries() < 64 + 16 + 1);
    }
}
