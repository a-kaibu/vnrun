//! The tables of the scripts' resources ('f' and 'e' entries of Arc01).
//!
//! An 'f' entry describes one layer (the n-th 'f' is layer n):
//!
//! ```text
//! i32 n, n x i32 picture       the layer's pictures (indexes into the 'b' pictures)
//! i32 a, i32 m, m x i32        (not used here)
//! i32 k, k x (x, y, z, i32)    its places: the top left corner and its depth
//! ```
//!
//! An 'e' entry is a menu of buttons (the title's is the first):
//!
//! ```text
//! i32 base layer, i32, i32, i32, i32 n
//! n x (i32 layer, i32 order, i32 x4, i32 k, k x i32 picture, i32 m, m x i32, i32, i32,
//!      i32 answer, i32 x7)
//! ```

fn ints(b: &[u8]) -> Vec<i32> {
    b.chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct LayerTable {
    pub pics: Vec<i32>,
    /// (x, y, depth)
    pub places: Vec<(i32, i32, i32)>,
}

impl LayerTable {
    pub fn parse(b: &[u8]) -> LayerTable {
        let v = ints(b);
        let get = |i: usize| v.get(i).copied().unwrap_or(0);
        let n = get(0).clamp(0, 10000) as usize;
        let pics = v.get(1..1 + n).unwrap_or(&[]).to_vec();
        let mut at = 1 + n + 1;
        let m = get(at).clamp(0, 10000) as usize;
        at += 1 + m;
        let k = get(at).clamp(0, 10000) as usize;
        at += 1;
        let places = (0..k)
            .map(|i| (get(at + 4 * i), get(at + 4 * i + 1), get(at + 4 * i + 2)))
            .collect();
        LayerTable { pics, places }
    }

    pub fn place(&self, i: i32) -> (i32, i32, i32) {
        self.places
            .get(i.max(0) as usize)
            .copied()
            .unwrap_or((0, 0, 0))
    }
}

#[derive(Debug, Clone)]
pub struct Button {
    pub layer: usize,
    /// pictures of the layer: one is shown under the pointer; of two, the first always and
    /// the second under the pointer
    pub pics: Vec<i32>,
    pub answer: i32,
}

#[derive(Debug, Clone, Default)]
pub struct ButtonSet {
    pub base: usize,
    pub buttons: Vec<Button>,
}

impl ButtonSet {
    pub fn parse(b: &[u8]) -> ButtonSet {
        let v = ints(b);
        let get = |i: usize| v.get(i).copied().unwrap_or(0);
        let base = get(0).max(0) as usize;
        let n = get(4).clamp(0, 100) as usize;
        let mut at = 5;
        let mut buttons = Vec::new();
        for _ in 0..n {
            let layer = get(at).max(0) as usize;
            let k = get(at + 6).clamp(0, 100) as usize;
            let pics = v.get(at + 7..at + 7 + k).unwrap_or(&[]).to_vec();
            let m_at = at + 7 + k;
            let m = get(m_at).clamp(0, 100) as usize;
            let answer = get(m_at + 1 + m + 2);
            buttons.push(Button {
                layer,
                pics,
                answer,
            });
            at = m_at + 1 + m + 3 + 7;
        }
        ButtonSet { base, buttons }
    }
}
