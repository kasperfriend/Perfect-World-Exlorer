//! Independent encoded-space reference for the recovered ordinary/mono glow.
//! This models UNORM8 pass boundaries, normalized bilinear clamp sampling and
//! offsets from 0x40edd0 / 0x40f053 (4 divided by FULL resolution).
//! Includes the recovered underwater-before-glow pass, not all special effects.
#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 3]>,
}
impl Image {
    pub fn pattern(width: usize, height: usize) -> Self {
        let pixels = (0..height)
            .flat_map(|y| {
                (0..width).map(move |x| {
                    [
                        ((17 * x + 29 * y) % 256) as u8,
                        ((71 * x + 13 * y + 31) % 256) as u8,
                        ((3 * x + 113 * y + 67) % 256) as u8,
                    ]
                })
            })
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let pixel = |xx: f32, yy: f32| {
            self.pixels[yy.clamp(0., (self.height - 1) as f32) as usize * self.width
                + xx.clamp(0., (self.width - 1) as f32) as usize]
        };
        let a = pixel(x0, y0);
        let b = pixel(x0 + 1., y0);
        let c = pixel(x0, y0 + 1.);
        let d = pixel(x0 + 1., y0 + 1.);
        std::array::from_fn(|k| {
            let top = a[k] as f32 + (b[k] as f32 - a[k] as f32) * fx;
            let bottom = c[k] as f32 + (d[k] as f32 - c[k] as f32) * fx;
            (top + (bottom - top) * fy) / 255.
        })
    }
    fn render(width: usize, height: usize, f: impl Fn(f32, f32) -> [f32; 3]) -> Self {
        let pixels = (0..height)
            .flat_map(|y| {
                let f = &f;
                (0..width).map(move |x| {
                    f(
                        (x as f32 + 0.5) / width as f32,
                        (y as f32 + 0.5) / height as f32,
                    )
                    .map(|c| (c.clamp(0., 1.) * 255.).round() as u8)
                })
            })
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }
    fn kernel(&self, u: f32, v: f32, dx: f32, dy: f32) -> [f32; 3] {
        let center = self.sample(u, v);
        let inner_a = self.sample(u - dx, v - dy);
        let inner_b = self.sample(u + dx, v + dy);
        let outer_a = self.sample(u - 2. * dx, v - 2. * dy);
        let outer_b = self.sample(u + 2. * dx, v + 2. * dy);
        std::array::from_fn(|k| {
            center[k] * 0.310
                + (inner_a[k] + inner_b[k]) * 0.238
                + (outer_a[k] + outer_b[k]) * 0.107
        })
    }
    /// Independently rasterize the procedural 256² bump texture and clamp-
    /// sample the encoded scene into a separate UNORM8 target before glow.
    pub fn underwater_warp(&self, seconds: f32) -> Self {
        let bump = |x: f32, y: f32| {
            let theta = std::f32::consts::TAU * 2. * ((x + y) / 256. - 1.);
            [
                (128. - (-32. * theta.cos()).trunc()) / 255.,
                (128. - (-32. * theta.sin()).trunc()) / 255.,
            ]
        };
        Self::render(self.width, self.height, |u, v| {
            let px = (u * 3.).fract() * 256. - 0.5;
            let py = (v * 3. * self.height as f32 / self.width as f32 + seconds * 0.5).fract()
                * 256.
                - 0.5;
            let x = px.floor();
            let y = py.floor();
            let fx = px - x;
            let fy = py - y;
            let a = bump(x, y);
            let b = bump(x + 1., y);
            let c = bump(x, y + 1.);
            let d = bump(x + 1., y + 1.);
            let normal: [f32; 2] = std::array::from_fn(|k| {
                let top = a[k] * (1. - fx) + b[k] * fx;
                let bottom = c[k] * (1. - fx) + d[k] * fx;
                (top * (1. - fy) + bottom * fy) * 2. - 1.
            });
            self.sample(u - normal[0] * 0.005, v + normal[1] * 0.005)
        })
    }
    pub fn bloom(&self, level: f32, power: f32, mono: [f32; 4]) -> Self {
        self.bloom_with_steps(
            level,
            power,
            mono,
            4. / self.width as f32,
            4. / self.height as f32,
        )
    }
    fn bloom_with_steps(&self, level: f32, power: f32, mono: [f32; 4], dx: f32, dy: f32) -> Self {
        let w = (self.width / 4).max(1);
        let h = (self.height / 4).max(1);
        let copy = Self::render(w, h, |u, v| self.sample(u, v));
        let horizontal = Self::render(w, h, |u, v| copy.kernel(u, v, dx, 0.));
        Self::render(self.width, self.height, |u, v| {
            let original = self.sample(u, v);
            let glow = horizontal.kernel(u, v, 0., dy);
            let base: [f32; 3] = std::array::from_fn(|k| {
                let add = if mono[3] > 0. { original[k] } else { glow[k] };
                original[k] + (glow[k] - original[k]) * level + power * glow[k] * add
            });
            if mono[3] <= 0. {
                return base;
            }
            let luma = (base[0] * 0.35 + base[1] * 0.6 + base[2] * 0.25).clamp(0., 1.);
            std::array::from_fn(|k| base[k] + (luma * mono[k] - base[k]) * mono[3])
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_image_preserved_by_copy_and_blur() {
        for (w, h) in [(64, 64), (23, 19), (67, 61)] {
            let image = Image {
                width: w,
                height: h,
                pixels: vec![[64, 128, 192]; w * h],
            };
            assert_eq!(image.bloom(1., 0., [0.; 4]).pixels, image.pixels);
        }
    }
    #[test]
    fn zero_strength_is_identity_even_for_odd_dimensions() {
        let image = Image::pattern(23, 19);
        assert_eq!(image.bloom(0., 0., [0.; 4]).pixels, image.pixels);
    }
    #[test]
    fn odd_size_fixture_rejects_the_old_inverse_quarter_dimensions() {
        let image = Image::pattern(23, 19);
        let recovered = image.bloom(0.25, 0.25, [0.; 4]);
        let legacy = image.bloom_with_steps(0.25, 0.25, [0.; 4], 1. / 5., 1. / 4.);
        let worst = recovered
            .pixels
            .iter()
            .zip(&legacy.pixels)
            .flat_map(|(a, b)| a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).abs()))
            .max()
            .unwrap();
        assert!(
            worst > 2,
            "fixture must reject legacy offsets beyond GPU tolerance, got {worst}"
        );
    }
    #[test]
    fn mono_uses_blur_times_original_not_blur_squared() {
        let image = Image::pattern(23, 19);
        let ordinary = image.bloom(0.5, 0.6, [0.; 4]);
        // A tiny positive mono coefficient selects monofullglow, but causes
        // negligible monochrome mixing; this isolates its different add term.
        let mono = image.bloom(0.5, 0.6, [1., 1., 1., 1e-8]);
        assert_ne!(ordinary.pixels, mono.pixels);
    }
    #[test]
    fn underwater_warp_is_a_quantized_input_to_glow_not_a_late_uv_change() {
        let image = Image::pattern(67, 61);
        let correct = image.underwater_warp(0.37).bloom(
            0.5,
            0.6,
            [187. / 255., 234. / 255., 187. / 255., 0.5],
        );
        let wrong = image
            .bloom(0.5, 0.6, [187. / 255., 234. / 255., 187. / 255., 0.5])
            .underwater_warp(0.37);
        let worst = correct
            .pixels
            .iter()
            .zip(&wrong.pixels)
            .flat_map(|(a, b)| a.iter().zip(b))
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        assert!(
            worst > 2,
            "pattern must distinguish the pass ordering, max difference {worst}"
        );
    }
}
