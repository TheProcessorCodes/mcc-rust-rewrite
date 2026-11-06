//! BXML, Reflex's binary XML (the `.dx9.database` files).
//!
//! A 36-byte header (nine u32: `BXML`, version, string count, pool offset,
//! pool size, attribute count, node count, unknown, packed size) and then
//! the body, zlib-packed when version bit 16 is set (otherwise the body
//! starts at byte 32). The body is the strings (NUL-terminated), a value
//! pool, attributes of 12 bytes (name, value, flag u16, type u16) and nodes
//! of 32 bytes (name, value, flag, type, first child, child count, first
//! attribute, attribute count). A value of type 1 is a string index; the
//! others are byte offsets into the pool (3 i32, 4 u32, 5 f32).

use std::io::Read;

use crate::Error;

#[derive(Clone, Copy)]
struct Attr {
    name: u32,
    value: u32,
    kind: u16,
}

#[derive(Clone, Copy)]
struct Node {
    name: u32,
    value: u32,
    kind: u32,
    first_child: u32,
    children: u32,
    first_attr: u32,
    attrs: u32,
}

pub struct Bxml {
    strings: Vec<String>,
    pool: Vec<u8>,
    attrs: Vec<Attr>,
    nodes: Vec<Node>,
}

fn u32_at(b: &[u8], p: usize) -> Result<u32, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::Format(format!("BXML ends at {p}")))
}

impl Bxml {
    pub fn parse(data: &[u8]) -> Result<Bxml, Error> {
        if data.get(0..4) != Some(b"BXML") {
            return Err(Error::Format("not BXML".into()));
        }
        let h: Vec<u32> = (0..9).map(|i| u32_at(data, i * 4)).collect::<Result<_, _>>()?;
        let raw = if h[1] & 0x10000 != 0 {
            let packed = data
                .get(36..36 + h[8] as usize)
                .ok_or_else(|| Error::Format("BXML body cut short".into()))?;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(packed).read_to_end(&mut out)?;
            out
        } else {
            data.get(32..).unwrap_or_default().to_vec()
        };
        let (count, pool_at, pool_size, attr_count, node_count) =
            (h[2] as usize, h[3] as usize, h[4] as usize, h[5] as usize, h[6] as usize);
        let strings = raw
            .get(..pool_at)
            .ok_or_else(|| Error::Format("BXML strings".into()))?
            .split(|&b| b == 0)
            .take(count)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let pool = raw
            .get(pool_at..pool_at + pool_size)
            .ok_or_else(|| Error::Format("BXML pool".into()))?
            .to_vec();
        let at = pool_at + pool_size;
        let mut attrs = Vec::with_capacity(attr_count);
        for i in 0..attr_count {
            let p = at + i * 12;
            let kind = raw.get(p + 10..p + 12).ok_or_else(|| Error::Format("BXML attrs".into()))?;
            attrs.push(Attr {
                name: u32_at(&raw, p)?,
                value: u32_at(&raw, p + 4)?,
                kind: u16::from_le_bytes([kind[0], kind[1]]),
            });
        }
        let at = at + attr_count * 12;
        let mut nodes = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let p = at + i * 32;
            nodes.push(Node {
                name: u32_at(&raw, p)?,
                value: u32_at(&raw, p + 4)?,
                kind: u32_at(&raw, p + 12)?,
                first_child: u32_at(&raw, p + 16)?,
                children: u32_at(&raw, p + 20)?,
                first_attr: u32_at(&raw, p + 24)?,
                attrs: u32_at(&raw, p + 28)?,
            });
        }
        Ok(Bxml {
            strings,
            pool,
            attrs,
            nodes,
        })
    }

    fn string(&self, i: u32) -> &str {
        self.strings.get(i as usize).map_or("", String::as_str)
    }

    pub fn name(&self, node: usize) -> &str {
        self.nodes.get(node).map_or("", |n| self.string(n.name))
    }

    /// A node's own text (value types 0 and 1 are string indices).
    pub fn text(&self, node: usize) -> Option<&str> {
        let n = self.nodes.get(node)?;
        (n.kind <= 1).then(|| self.string(n.value))
    }

    /// The names of a node's attributes, in order.
    pub fn attr_names(&self, node: usize) -> impl Iterator<Item = &str> + '_ {
        let n = self.nodes.get(node).copied();
        n.into_iter()
            .flat_map(|n| n.first_attr..n.first_attr + n.attrs)
            .filter_map(|i| self.attrs.get(i as usize))
            .map(|a| self.string(a.name))
    }

    pub fn children(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        let n = self.nodes.get(node).copied();
        n.into_iter()
            .flat_map(|n| n.first_child as usize..(n.first_child + n.children) as usize)
            .filter(|&c| c < self.nodes.len())
    }

    /// The children named `name`.
    pub fn find_all<'a>(&'a self, node: usize, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.children(node).filter(move |&c| self.name(c) == name)
    }

    /// Every node named `name`, anywhere.
    pub fn all_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        (0..self.nodes.len()).filter(move |&n| self.name(n) == name)
    }

    pub fn find(&self, node: usize, name: &str) -> Option<usize> {
        self.find_all(node, name).next()
    }

    fn attr(&self, node: usize, key: &str) -> Option<Attr> {
        let n = self.nodes.get(node)?;
        (n.first_attr..n.first_attr + n.attrs)
            .filter_map(|i| self.attrs.get(i as usize))
            .find(|a| self.string(a.name) == key)
            .copied()
    }

    pub fn attr_str(&self, node: usize, key: &str) -> Option<&str> {
        let a = self.attr(node, key)?;
        (a.kind == 1).then(|| self.string(a.value))
    }

    /// An integer attribute (types 3 and 4).
    pub fn attr_u64(&self, node: usize, key: &str) -> Option<u64> {
        let a = self.attr(node, key)?;
        let p = a.value as usize;
        let v = u32::from_le_bytes(self.pool.get(p..p + 4)?.try_into().ok()?);
        match a.kind {
            3 => Some(v as i32 as i64 as u64),
            4 => Some(v as u64),
            _ => None,
        }
    }
}
