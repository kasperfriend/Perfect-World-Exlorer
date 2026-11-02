//! Compare production LUT words with the separate Windows-x86 MSVCRT capture.
#![allow(dead_code)]
#[path = "../src/gamma_curve.rs"]
mod gamma_curve;
fn main() {
    let path = std::env::args().nth(1).expect("native PC24 word file");
    let data = std::fs::read(path).expect("read native words");
    assert_eq!(data.len(), 101 * 256 * 2);
    let mut mismatches = 0;
    let mut hash = 0xcbf29ce484222325u64;
    for setting in 0..=100 {
        for index in 0..=255usize {
            let pos = (setting * 256 + index) * 2;
            let native = u16::from_le_bytes([data[pos], data[pos + 1]]);
            let actual = gamma_curve::entry(index as u8, setting as f32);
            if native != actual {
                if mismatches < 16 {
                    eprintln!(
                        "setting={setting} index={index}: native={native} production={actual}"
                    );
                }
                mismatches += 1;
            }
            for byte in native.to_le_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
            }
        }
    }
    assert_eq!(mismatches, 0, "native x86 MSVCRT/production LUT mismatches");
    println!(
        "::notice::25856 production gamma words match native x86 PC24 MSVCRT, FNV={hash:016x}"
    );
}
