//! Experimental translation of the client's tree wind-strength noise.
//!
//! This does not reproduce SpeedTree's leaf rocking and is not wired to the
//! renderer. The existing cache contains sampled positions rather than the
//! runtime parameters needed to apply this signal faithfully.

#[derive(Clone)]
pub struct ClientWind {
    table: [f32; 256],
    offsets: [u32; 8],
    weights: [f32; 8],
}

impl ClientWind {
    /// Client initialization at elementclient.exe 0x64fcbe and 0x766320.
    pub fn new() -> Self {
        let mut seed = 0x12345_u32;
        let mut raw = [[0.0_f32; 256]; 3];
        for component in &mut raw {
            for value in component {
                let number = next_random(&mut seed) % 2001;
                *value = ((number as i32 - 1000) as f64 * f64::from(0.001_f32)) as f32;
            }
        }
        let mut table = [0.0; 256];
        for i in 0..256 {
            table[i] = ((f64::from(raw[0][(i + 255) % 256])
                + f64::from(raw[0][(i + 1) % 256])) * 0.25
                + f64::from(raw[0][i]) * 0.5) as f32;
        }
        let persistence = f32::from_bits(0x3f7fbe77);
        let mut weights = [0.0_f32; 8];
        let mut total = 0.0_f32;
        let mut weight = 1.0_f32;
        let mut offsets = [0; 8];
        for i in 0..8 {
            weights[i] = weight;
            total += weight;
            weight *= persistence;
            offsets[i] = next_random(&mut seed) % 1023;
        }
        for weight in &mut weights {
            *weight = (f64::from(*weight) / f64::from(total)) as f32;
        }
        Self { table, offsets, weights }
    }

    /// Evaluate at client-global seconds. `None` denotes inputs for which
    /// x87 integer conversion or the translation's behavior is unverified.
    /// f64 intermediates approximate x87 precision rather than reproduce it.
    pub fn sample(&self, time: f32) -> Option<f32> {
        if !time.is_finite() || time < 0.0 || time > 1_000_000.0 {
            return None;
        }
        let mut result = 0.0_f32;
        for i in 0..8 {
            let period = 256_u32 >> i;
            let coordinate = f64::from(time) / f64::from(period) + f64::from(self.offsets[i]);
            let index = coordinate.trunc() as usize;
            let fraction = (coordinate - index as f64) as f32;
            let a = f64::from(self.table[index % 256]);
            let b = f64::from(self.table[(index + 1) % 256]);
            result = (f64::from(result) + (a * (1.0 - f64::from(fraction))
                + b * f64::from(fraction)).abs() * f64::from(self.weights[i])) as f32;
        }
        Some(result)
    }
}

fn next_random(seed: &mut u32) -> u32 {
    *seed = (u64::from(*seed) * 16807 % 2_147_483_647) as u32;
    *seed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulator_fixtures_and_period() {
        // From isolated Unicorn execution of the identified client EXE.
        let wind = ClientWind::new();
        assert_eq!(wind.offsets, [215, 370, 417, 434, 952, 739, 9, 779]);
        for (time, expected) in [
            (0.0, 0.386193573), (0.125, 0.385221481), (1.0, 0.378416777),
            (5.0, 0.399008989), (16.0, 0.356702805), (64.0, 0.353442341),
            (256.0, 0.286081165), (1024.0, 0.194191962), (65536.0, 0.386193573),
        ] {
            assert!((wind.sample(time).unwrap() - expected).abs() < 0.000001);
        }
        assert_eq!(wind.sample(-1.0), None);
        assert_eq!(wind.sample(f32::NAN), None);
    }
}
