//! RGBA to I420 (BT.601, limited range), the encoder's input format.
//! Integer maths over row pairs in parallel: about ten times faster than
//! OpenH264's float converter for full-HD frames.

use openh264::formats::YUVSlices;
use rayon::prelude::*;

#[derive(Default)]
pub struct I420 {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    width: usize,
    height: usize,
}

impl I420 {
    /// Converts tightly packed RGBA; `width` and `height` must be even.
    pub fn convert(&mut self, rgba: &[u8], width: usize, height: usize) {
        debug_assert!(width.is_multiple_of(2) && height.is_multiple_of(2));
        debug_assert_eq!(rgba.len(), width * height * 4);
        let (cw, ch) = (width / 2, height / 2);
        self.y.resize(width * height, 0);
        self.u.resize(cw * ch, 0);
        self.v.resize(cw * ch, 0);
        (self.width, self.height) = (width, height);

        self.y
            .par_chunks_mut(width * 2)
            .zip(self.u.par_chunks_mut(cw))
            .zip(self.v.par_chunks_mut(cw))
            .zip(rgba.par_chunks(width * 8))
            .for_each(|(((y, u), v), src)| {
                let (top, bottom) = src.split_at(width * 4);
                for cx in 0..cw {
                    let (mut r, mut g, mut b) = (0, 0, 0);
                    for (row, line) in [top, bottom].into_iter().enumerate() {
                        for dx in 0..2 {
                            let x = cx * 2 + dx;
                            let p = &line[x * 4..x * 4 + 3];
                            let (pr, pg, pb) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                            y[row * width + x] = luma(pr, pg, pb);
                            (r, g, b) = (r + pr, g + pg, b + pb);
                        }
                    }
                    // Average of the 2×2 block, rounded.
                    let (r, g, b) = ((r + 2) >> 2, (g + 2) >> 2, (b + 2) >> 2);
                    u[cx] = clamp(((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128);
                    v[cx] = clamp(((112 * r - 94 * g - 18 * b + 128) >> 8) + 128);
                }
            });
    }

    pub fn slices(&self) -> YUVSlices<'_> {
        let cw = self.width / 2;
        YUVSlices::new((&self.y, &self.u, &self.v), (self.width, self.height), (self.width, cw, cw))
    }
}

fn luma(r: i32, g: i32, b: i32) -> u8 {
    clamp(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16)
}

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::formats::{RgbaSliceU8, YUVBuffer, YUVSource};

    #[test]
    fn matches_openh264_within_rounding() {
        let (w, h) = (64, 32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255]).collect();
        let reference = YUVBuffer::from_rgb_source(RgbaSliceU8::new(&rgba, (w, h)));
        let mut ours = I420::default();
        ours.convert(&rgba, w, h);
        let ours = ours.slices();
        for (name, a, b) in [("y", ours.y(), reference.y()), ("u", ours.u(), reference.u()), ("v", ours.v(), reference.v())] {
            let worst = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
            assert!(worst <= 2, "{name} plane differs by up to {worst}");
        }
    }

    #[test]
    fn white_and_black_hit_the_limits() {
        let mut out = I420::default();
        out.convert(&[255; 2 * 2 * 4], 2, 2);
        assert_eq!((out.y[0], out.u[0], out.v[0]), (235, 128, 128));
        out.convert(&[0, 0, 0, 255].repeat(4), 2, 2);
        assert_eq!((out.y[0], out.u[0], out.v[0]), (16, 128, 128));
    }
}
