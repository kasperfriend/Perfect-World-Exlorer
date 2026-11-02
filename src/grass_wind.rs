//! Client-derived CELGrassLand noise and 80-frame wind history.
//! The integration follows the traced spring structure; exact timing and
//! floating point behavior have not yet been compared with the live client.
const LEN: usize = 256;
pub struct GrassWind {
    table: [f32; LEN],
    offsets: [usize; 8],
    weights: [f32; 8],
    time: f32,
    pos: [f32; 3],
    vel: [f32; 3],
    history: [[f32; 4]; 80],
}
impl GrassWind {
    pub fn new() -> Self {
        let mut seed: u64 = 0x12345;
        let mut next = || {
            seed = seed * 16807 % 2147483647;
            seed as usize
        };
        let mut table = [0.; LEN];
        for channel in 0..3 {
            let mut raw = [0.; LEN];
            for x in &mut raw {
                *x = ((next() % 2001) as i32 - 1000) as f32 * 0.001;
            }
            if channel == 0 {
                for i in 0..LEN {
                    table[i] =
                        (raw[(i + LEN - 1) % LEN] + raw[(i + 1) % LEN]) * 0.25 + raw[i] * 0.5;
                }
            }
        }
        let mut offsets = [0; 8];
        let mut weights = [0.; 8];
        let mut w = 1.;
        let mut total = 0.;
        for i in 0..8 {
            offsets[i] = next() % 1023;
            weights[i] = w;
            total += w;
            w *= 0.95;
        }
        for weight in &mut weights {
            *weight /= total;
        }
        Self {
            table,
            offsets,
            weights,
            time: 0.,
            pos: [0.; 3],
            vel: [0.; 3],
            history: [[0.; 4]; 80],
        }
    }
    fn noise(&self, time: f32) -> f32 {
        let mut result = 0.;
        for i in 0..8 {
            let p = time / (256 >> i) as f32 + self.offsets[i] as f32;
            let base = p.floor();
            let frac = p - base;
            let idx = (base as i64).rem_euclid(LEN as i64) as usize;
            let value = self.table[idx] * (1. - frac) + self.table[(idx + 1) % LEN] * frac;
            result += self.weights[i] * value.abs();
        }
        result
    }
    pub fn update(&mut self, elapsed: f32) {
        // The game receives milliseconds and shifts once per update. Limit a
        // suspend/debugger pause to avoid destabilizing an explicit spring.
        let dt = (elapsed * 1000.).floor().clamp(0., 100.) * 0.001;
        self.time += dt;
        let force = self.noise(self.time * 20.);
        let forcing = [force, -0.5 * force, 0.];
        for (i, &external) in forcing.iter().enumerate() {
            let predicted = self.pos[i] + self.vel[i] * dt;
            let acceleration = (external - 2. * predicted - 0.1 * self.vel[i]) / 0.1;
            self.vel[i] += 0.5 * acceleration * dt;
            self.pos[i] += self.vel[i] * dt;
        }
        self.history.copy_within(1..80, 0);
        self.history[79] = [self.pos[0], self.pos[1], self.pos[2], 0.];
    }
    pub fn history(&self) -> [[f32; 4]; 80] {
        self.history
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_and_bounded() {
        let mut a = GrassWind::new();
        let mut b = GrassWind::new();
        for _ in 0..1000 {
            a.update(0.016);
            b.update(0.016);
        }
        assert_eq!(a.history(), b.history());
        assert!(a.history().iter().any(|v| v[0].abs() > 0.001));
        assert!(a
            .history()
            .iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() < 2.));
    }
    #[test]
    fn history_shifts_and_pause_is_safe() {
        let mut a = GrassWind::new();
        a.update(0.016);
        let last = a.history()[79];
        a.update(0.016);
        assert_eq!(a.history()[78], last);
        a.update(10000.);
        assert!(a.history().iter().flatten().all(|v| v.is_finite()));
    }
}
