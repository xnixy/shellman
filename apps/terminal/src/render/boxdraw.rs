//! Box-drawing lines, blocks and Powerline shapes drawn here, cell-sized, instead of taken from
//! the font: a font's glyphs for them rarely fill the cell exactly at every size, which leaves
//! gaps between cells of a frame or a prompt's segments. Each is a coverage bitmap exactly one
//! cell big, so neighbours always join.
//!
//! Lines are described by their four arms (up, right, down, left), each none, light, heavy or
//! double; dashed lines and diagonals are left to the font.

/// An arm's weight.
const N: u8 = 0;
const L: u8 = 1;
const H: u8 = 2;
const D: u8 = 3;

/// Arms of U+2500..=U+257F, in order up, right, down, left; None where the font draws it.
fn arms(c: char) -> Option<[u8; 4]> {
    let i = (c as u32).checked_sub(0x2500)? as usize;
    const T: [Option<[u8; 4]>; 128] = {
        let mut t: [Option<[u8; 4]>; 128] = [None; 128];
        macro_rules! set { ($($i:expr => $a:expr),* $(,)?) => { $( t[$i] = Some($a); )* } }
        set! {
            0x00 => [N,L,N,L], 0x01 => [N,H,N,H], 0x02 => [L,N,L,N], 0x03 => [H,N,H,N],
            0x0C => [N,L,L,N], 0x0D => [N,H,L,N], 0x0E => [N,L,H,N], 0x0F => [N,H,H,N],
            0x10 => [N,N,L,L], 0x11 => [N,N,L,H], 0x12 => [N,N,H,L], 0x13 => [N,N,H,H],
            0x14 => [L,L,N,N], 0x15 => [L,H,N,N], 0x16 => [H,L,N,N], 0x17 => [H,H,N,N],
            0x18 => [L,N,N,L], 0x19 => [L,N,N,H], 0x1A => [H,N,N,L], 0x1B => [H,N,N,H],
            0x1C => [L,L,L,N], 0x1D => [L,H,L,N], 0x1E => [H,L,L,N], 0x1F => [L,L,H,N],
            0x20 => [H,L,H,N], 0x21 => [H,H,L,N], 0x22 => [L,H,H,N], 0x23 => [H,H,H,N],
            0x24 => [L,N,L,L], 0x25 => [L,N,L,H], 0x26 => [H,N,L,L], 0x27 => [L,N,H,L],
            0x28 => [H,N,H,L], 0x29 => [H,N,L,H], 0x2A => [L,N,H,H], 0x2B => [H,N,H,H],
            0x2C => [N,L,L,L], 0x2D => [N,L,L,H], 0x2E => [N,H,L,L], 0x2F => [N,H,L,H],
            0x30 => [N,L,H,L], 0x31 => [N,L,H,H], 0x32 => [N,H,H,L], 0x33 => [N,H,H,H],
            0x34 => [L,L,N,L], 0x35 => [L,L,N,H], 0x36 => [L,H,N,L], 0x37 => [L,H,N,H],
            0x38 => [H,L,N,L], 0x39 => [H,L,N,H], 0x3A => [H,H,N,L], 0x3B => [H,H,N,H],
            0x3C => [L,L,L,L], 0x3D => [L,L,L,H], 0x3E => [L,H,L,L], 0x3F => [L,H,L,H],
            0x40 => [H,L,L,L], 0x41 => [L,L,H,L], 0x42 => [H,L,H,L], 0x43 => [H,L,L,H],
            0x44 => [H,H,L,L], 0x45 => [L,L,H,H], 0x46 => [L,H,H,L], 0x47 => [H,H,L,H],
            0x48 => [L,H,H,H], 0x49 => [H,L,H,H], 0x4A => [H,H,H,L], 0x4B => [H,H,H,H],
            0x50 => [N,D,N,D], 0x51 => [D,N,D,N], 0x52 => [N,D,L,N], 0x53 => [N,L,D,N],
            0x54 => [N,D,D,N], 0x55 => [N,N,L,D], 0x56 => [N,N,D,L], 0x57 => [N,N,D,D],
            0x58 => [L,D,N,N], 0x59 => [D,L,N,N], 0x5A => [D,D,N,N], 0x5B => [L,N,N,D],
            0x5C => [D,N,N,L], 0x5D => [D,N,N,D], 0x5E => [L,D,L,N], 0x5F => [D,L,D,N],
            0x60 => [D,D,D,N], 0x61 => [L,N,L,D], 0x62 => [D,N,D,L], 0x63 => [D,N,D,D],
            0x64 => [N,D,L,D], 0x65 => [N,L,D,L], 0x66 => [N,D,D,D], 0x67 => [L,D,N,D],
            0x68 => [D,L,N,L], 0x69 => [D,D,N,D], 0x6A => [L,D,L,D], 0x6B => [D,L,D,L],
            0x6C => [D,D,D,D],
            0x74 => [N,N,N,L], 0x75 => [L,N,N,N], 0x76 => [N,L,N,N], 0x77 => [N,N,L,N],
            0x78 => [N,N,N,H], 0x79 => [H,N,N,N], 0x7A => [N,H,N,N], 0x7B => [N,N,H,N],
            0x7C => [N,H,N,L], 0x7D => [L,N,H,N], 0x7E => [N,L,N,H], 0x7F => [H,N,L,N],
        }
        t
    };
    *T.get(i)?
}

/// Whether `c` is drawn here.
pub fn handles(c: char) -> bool {
    match c as u32 {
        0x2500..=0x257F => arms(c).is_some() || (0x256D..=0x2570).contains(&(c as u32)),
        0x2580..=0x259F => true,
        0xE0B0..=0xE0B4 | 0xE0B6 => true,
        _ => false,
    }
}

/// A coverage canvas one cell big.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![0; w * h] }
    }

    /// Fill the pixels whose centres are inside `[x0, x1) x [y0, y1)`.
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, v: u8) {
        let col = |x: f32| ((x - 0.5).ceil().max(0.0) as usize).min(self.w);
        let row = |y: f32| ((y - 0.5).ceil().max(0.0) as usize).min(self.h);
        let (a, b, c, d) = (col(x0), col(x1), row(y0), row(y1));
        for y in c..d {
            for x in a..b {
                let p = &mut self.px[y * self.w + x];
                *p = (*p).max(v);
            }
        }
    }

    /// Fill by a test on each pixel, 4x4 samples a pixel, for smooth edges.
    fn shape(&mut self, inside: impl Fn(f32, f32) -> bool) {
        for y in 0..self.h {
            for x in 0..self.w {
                let mut n = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        if inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) {
                            n += 1;
                        }
                    }
                }
                let p = &mut self.px[y * self.w + x];
                *p = (*p).max((n * 255 / 16) as u8);
            }
        }
    }
}

/// The bitmap for `c` in a `w` x `h` cell, lines `thin` pixels thick; None for what the font draws.
pub fn draw(c: char, w: usize, h: usize, thin: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || !handles(c) {
        return None;
    }
    let mut cv = Canvas::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let u = c as u32;
    match u {
        0x2580..=0x259F => blocks(&mut cv, u, wf, hf),
        0x256D..=0x2570 => {
            let t = thin as f32;
            let (cx, cy) = (wf / 2.0, hf / 2.0);
            let r = cx.min(cy);
            // The arc's centre, toward the corner the lines leave from.
            let (sx, sy) = match u {
                0x256D => (1.0, 1.0),   // ╭ right and down
                0x256E => (-1.0, 1.0),  // ╮ left and down
                0x256F => (-1.0, -1.0), // ╯ left and up
                _ => (1.0, -1.0),       // ╰ right and up
            };
            let (ox, oy) = (cx + sx * r, cy + sy * r);
            cv.shape(|x, y| {
                let (dx, dy) = (x - ox, y - oy);
                // Only the quarter facing the cell's centre.
                if dx * sx > 0.0 || dy * sy > 0.0 {
                    return false;
                }
                ((dx * dx + dy * dy).sqrt() - r).abs() <= t / 2.0
            });
            // Straight on from the arc's ends to the edges.
            let half = t / 2.0;
            if sx > 0.0 {
                cv.rect(ox, cy - half, wf, cy + half, 255);
            } else {
                cv.rect(0.0, cy - half, ox, cy + half, 255);
            }
            if sy > 0.0 {
                cv.rect(cx - half, oy, cx + half, hf, 255);
            } else {
                cv.rect(cx - half, 0.0, cx + half, oy, 255);
            }
        }
        0x2500..=0x257F => lines(&mut cv, arms(c)?, wf, hf, thin as f32),
        _ => powerline(&mut cv, u, wf, hf, thin as f32),
    }
    Some(cv.px)
}

fn lines(cv: &mut Canvas, a: [u8; 4], w: f32, h: f32, t: f32) {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let heavy = (t * 2.0).max(t + 1.0);
    // A double line's two strokes sit this far either side of the centre line.
    let g = t.max(1.0);
    let thick = |k: u8| match k {
        L => t,
        H => heavy,
        D => 2.0 * g + t,
        _ => 0.0,
    };
    let [up, right, down, left] = a;
    let vert = thick(up).max(thick(down));
    let horz = thick(left).max(thick(right));
    // Each arm: its weight, the arms either side of it (negative side first) and its opposite.
    // Directions: 0 up, 1 right, 2 down, 3 left.
    for dir in 0..4 {
        let k = a[dir];
        if k == N {
            continue;
        }
        let vertical = dir % 2 == 0;
        // The perpendicular arms: for a vertical arm, left (negative x) and right; for a
        // horizontal one, up (negative y) and down.
        let (neg, pos) = if vertical { (left, right) } else { (up, down) };
        let opposite = a[(dir + 2) % 4];
        let across = if vertical { horz } else { vert };
        // Distance from the centre, along the arm, at which a stroke starts (negative: it
        // reaches past the centre), for a stroke at offset `off` across the arm.
        let stop = |side_arm: u8, other_arm: u8| -> f32 {
            if k != D {
                return -across / 2.0;
            }
            if side_arm != N {
                if side_arm == D { g - t / 2.0 } else { thick(side_arm) / 2.0 }
            } else if opposite != N {
                -t / 2.0
            } else if other_arm != N {
                -(g + t / 2.0)
            } else {
                -t / 2.0
            }
        };
        let strokes: &[(f32, f32)] = if k == D {
            &[(-g, stop(neg, pos)), (g, stop(pos, neg))]
        } else {
            &[(0.0, stop(N, N))]
        };
        let width = if k == D { t } else { thick(k) };
        for &(off, from) in strokes {
            // Rectangle from the centre (+ from) out to the edge, `width` thick around `off`.
            match dir {
                0 => cv.rect(cx + off - width / 2.0, 0.0, cx + off + width / 2.0, cy - from, 255),
                2 => cv.rect(cx + off - width / 2.0, cy + from, cx + off + width / 2.0, h, 255),
                1 => cv.rect(cx + from, cy + off - width / 2.0, w, cy + off + width / 2.0, 255),
                _ => cv.rect(0.0, cy + off - width / 2.0, cx - from, cy + off + width / 2.0, 255),
            }
        }
    }
}

fn blocks(cv: &mut Canvas, u: u32, w: f32, h: f32) {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let eighth = |n: u32, of: f32| (of * n as f32 / 8.0).round();
    match u {
        0x2580 => cv.rect(0.0, 0.0, w, cy.round(), 255),
        0x2581..=0x2587 => cv.rect(0.0, h - eighth(u - 0x2580, h), w, h, 255),
        0x2588 => cv.rect(0.0, 0.0, w, h, 255),
        0x2589..=0x258F => cv.rect(0.0, 0.0, eighth(0x2590 - u, w), h, 255),
        0x2590 => cv.rect(cx.round(), 0.0, w, h, 255),
        0x2591 => cv.rect(0.0, 0.0, w, h, 64),
        0x2592 => cv.rect(0.0, 0.0, w, h, 128),
        0x2593 => cv.rect(0.0, 0.0, w, h, 192),
        0x2594 => cv.rect(0.0, 0.0, w, eighth(1, h), 255),
        0x2595 => cv.rect(w - eighth(1, w), 0.0, w, h, 255),
        _ => {
            // Quadrants: upper left, upper right, lower left, lower right.
            let q: [bool; 4] = match u {
                0x2596 => [false, false, true, false],
                0x2597 => [false, false, false, true],
                0x2598 => [true, false, false, false],
                0x2599 => [true, false, true, true],
                0x259A => [true, false, false, true],
                0x259B => [true, true, true, false],
                0x259C => [true, true, false, true],
                0x259D => [false, true, false, false],
                0x259E => [false, true, true, false],
                _ => [false, true, true, true],
            };
            let (mx, my) = (cx.round(), cy.round());
            let parts = [(0.0, 0.0, mx, my), (mx, 0.0, w, my), (0.0, my, mx, h), (mx, my, w, h)];
            for (on, (x0, y0, x1, y1)) in q.iter().zip(parts) {
                if *on {
                    cv.rect(x0, y0, x1, y1, 255);
                }
            }
        }
    }
}

fn powerline(cv: &mut Canvas, u: u32, w: f32, h: f32, t: f32) {
    let cy = h / 2.0;
    match u {
        // Solid triangles, pointing right and left.
        0xE0B0 => cv.shape(|x, y| x <= w * (1.0 - (y - cy).abs() / cy)),
        0xE0B2 => cv.shape(|x, y| w - x <= w * (1.0 - (y - cy).abs() / cy)),
        // Thin chevrons: the triangle's two sloped sides, `t` thick.
        0xE0B1 | 0xE0B3 => {
            let len = (w * w + cy * cy).sqrt();
            cv.shape(|x, y| {
                let x = if u == 0xE0B1 { x } else { w - x };
                // Distance to the line from (0, 0) or (0, h) to (w, cy).
                let dy = (y - cy).abs();
                let d = (cy * x + w * dy - w * cy).abs() / len;
                d <= t / 2.0 + 0.3 && x <= w
            })
        }
        // Half discs, the flat side on the left (E0B4) or right (E0B6).
        _ => {
            let r = cy;
            cv.shape(|x, y| {
                let dx = if u == 0xE0B4 { x } else { w - x };
                let rx = w.min(r);
                (dx / rx).powi(2) + ((y - cy) / r).powi(2) <= 1.0
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(px: &[u8], w: usize, x: usize, y: usize) -> u8 {
        px[y * w + x]
    }

    #[test]
    fn horizontal_and_vertical_lines_reach_every_edge() {
        let (w, h) = (9, 19);
        let horiz = draw('─', w, h, 1).unwrap();
        assert!(at(&horiz, w, 0, h / 2) == 255 && at(&horiz, w, w - 1, h / 2) == 255);
        assert_eq!(at(&horiz, w, 4, 0), 0);
        let vert = draw('│', w, h, 1).unwrap();
        assert!(at(&vert, w, w / 2, 0) == 255 && at(&vert, w, w / 2, h - 1) == 255);
        // A cross joins all four.
        let cross = draw('┼', w, h, 1).unwrap();
        for (x, y) in [(0, h / 2), (w - 1, h / 2), (w / 2, 0), (w / 2, h - 1), (w / 2, h / 2)] {
            assert_eq!(at(&cross, w, x, y), 255, "{x},{y}");
        }
    }

    #[test]
    fn a_corner_has_only_its_two_arms() {
        let (w, h) = (10, 20);
        let c = draw('┌', w, h, 2).unwrap();
        assert_eq!(at(&c, w, w - 1, h / 2), 255);
        assert_eq!(at(&c, w, w / 2, h - 1), 255);
        assert_eq!(at(&c, w, 0, h / 2), 0);
        assert_eq!(at(&c, w, w / 2, 0), 0);
    }

    #[test]
    fn double_lines_are_two_strokes_with_a_gap() {
        let (w, h) = (11, 21);
        let d = draw('═', w, h, 1).unwrap();
        let col: Vec<u8> = (0..h).map(|y| at(&d, w, 0, y)).collect();
        let strokes = col.windows(2).filter(|p| p[0] == 0 && p[1] == 255).count();
        assert_eq!(strokes, 2, "{col:?}");
        // A double corner's outer stroke turns the corner: ╔ has ink at the inner corner's
        // outside (up-left of centre) and none past it.
        let c = draw('╔', w, h, 1).unwrap();
        assert_eq!(at(&c, w, w / 2 - 1, h / 2 - 1), 255);
        assert_eq!(at(&c, w, 0, h / 2 - 1), 0);
    }

    #[test]
    fn blocks_fill_their_part() {
        let (w, h) = (8, 16);
        let full = draw('█', w, h, 1).unwrap();
        assert!(full.iter().all(|&v| v == 255));
        let lower = draw('▄', w, h, 1).unwrap();
        assert_eq!(at(&lower, w, 0, 0), 0);
        assert_eq!(at(&lower, w, 0, h - 1), 255);
        assert_eq!(lower.iter().filter(|&&v| v == 255).count(), w * h / 2);
        let shade = draw('▒', w, h, 1).unwrap();
        assert!(shade.iter().all(|&v| v == 128));
    }

    #[test]
    fn powerline_triangle_fills_the_left_edge_and_points_right() {
        let (w, h) = (10, 20);
        let tri = draw('\u{E0B0}', w, h, 1).unwrap();
        assert!((2..h - 2).all(|y| at(&tri, w, 0, y) > 100), "the flat side is the whole edge");
        assert!(at(&tri, w, 0, 0) > 0 && at(&tri, w, 0, h - 1) > 0);
        assert!(at(&tri, w, w - 1, h / 2) > 0);
        assert_eq!(at(&tri, w, w - 1, 0), 0);
    }

    #[test]
    fn dashes_and_text_are_the_fonts() {
        assert!(draw('┄', 8, 16, 1).is_none());
        assert!(draw('a', 8, 16, 1).is_none());
        assert!(draw('╱', 8, 16, 1).is_none());
        assert!(handles('╭') && handles('▓') && handles('\u{E0B2}'));
    }
}
