//! Rule wipes: a grey rule picture stretched over the screen, its dark parts going (or
//! coming) first.

use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

/// A wipe by a rule picture (a grey mask stretched over the screen): at `progress` 0..1 the
/// screen pixels whose rule brightness r (0..1) is below the moving edge have gone (or, with
/// `appear`, have come), with a soft edge `vague` wide (0 hard, 1 a whole fade). The draws of a
/// transition that leave carry it as is; the ones that arrive carry it with `appear`.
///
/// The browser compositor (web/src/compositor.ts) does the same per pixel with the rule's red
/// channel; the web crates hand rules over with [`rule_fields`].
#[derive(Debug, Clone)]
pub struct Rule {
    pub image: Rc<Image>,
    pub progress: f32,
    pub vague: f32,
    pub appear: bool,
}

impl Rule {
    /// How much of a pixel with rule brightness `r` (0..1) is shown.
    pub fn mask(&self, r: f32) -> f32 {
        let v = self.vague.clamp(1.0 / 255.0, 1.0);
        // the edge moves from -v (nothing gone) to 1 (all gone)
        let edge = self.progress.clamp(0.0, 1.0) * (1.0 + v) - v;
        let keep = ((r - edge) / v).clamp(0.0, 1.0);
        if self.appear { 1.0 - keep } else { keep }
    }

    /// The rule brightness at a screen pixel of a `w` x `h` screen.
    pub fn at(&self, x: u32, y: u32, w: u32, h: u32) -> f32 {
        let i = &self.image;
        if i.w == 0 || i.h == 0 || w == 0 || h == 0 {
            return 1.0;
        }
        let (rx, ry) = (
            (x as u64 * i.w as u64 / w as u64) as i32,
            (y as u64 * i.h as u64 / h as u64) as i32,
        );
        i.at(rx, ry)[0] as f32 / 255.0
    }
}

/// Numbers per rule in a web crate's `rules()`: the draw's index, the rule picture's id, width,
/// height, progress, vague and appear (1 / 0).
pub const RULE_FIELDS: usize = 7;

/// The draws on a black `w` x `h` picture, each with its rule (by draw index) if any.
pub fn render_ruled(w: u32, h: u32, ds: &[Draw], rules: &[(usize, Rule)]) -> Image {
    let mut img = Image::new(w, h);
    img.fill([0, 0, 0, 255]);
    for (i, d) in ds.iter().enumerate() {
        let r = rules.iter().find(|r| r.0 == i).map(|r| &r.1);
        draw_ruled(&mut img, &d.image, d.src, d.dst, &d.place, r);
    }
    img
}

/// The numbers [`RULE_FIELDS`] for rules of the draws (by index).
pub fn rule_fields(rules: &[(usize, Rule)]) -> Vec<f32> {
    let mut out = Vec::with_capacity(rules.len() * RULE_FIELDS);
    for (i, r) in rules {
        out.extend([
            *i as f32,
            r.image.id as f32,
            r.image.w as f32,
            r.image.h as f32,
            r.progress,
            r.vague,
            r.appear as u8 as f32,
        ]);
    }
    out
}

/// Draws `src[sr]` so that it covers `dst` on `out` (nexas::gfx::draw), through a rule wipe.
pub fn draw_ruled(
    out: &mut Image,
    src: &Image,
    sr: [i32; 4],
    dst: [f32; 4],
    p: &Place,
    rule: Option<&Rule>,
) {
    let (w, h) = (out.w, out.h);
    match rule {
        Some(r) => {
            let mask = |x: u32, y: u32| r.mask(r.at(x, y, w, h));
            nexas::gfx::draw_masked(out, src, sr, dst, p, Some(&mask));
        }
        None => nexas::gfx::draw_masked(out, src, sr, dst, p, None),
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;

    #[test]
    fn rule_mask() {
        let r = |p: f32, appear: bool| Rule {
            image: Rc::new(Image::new(1, 1)),
            progress: p,
            vague: 0.25,
            appear,
        };
        // nothing gone at the start, everything at the end
        assert_eq!(r(0.0, false).mask(0.0), 1.0);
        assert_eq!(r(1.0, false).mask(1.0), 0.0);
        // dark parts go first
        assert!(r(0.5, false).mask(0.1) < r(0.5, false).mask(0.9));
        assert_eq!(r(0.5, true).mask(0.1), 1.0 - r(0.5, false).mask(0.1));
    }
}
