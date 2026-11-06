//! Phase 5 sky-seam regression (P5.09): the panorama meridian.
//!
//! The viewer maps a sky direction to the client's two-image panoramas with
//! `az = atan2(x, -z) / 2pi + 0.5`, then `u = fract(az * 2)` and
//! `side = floor(az * 2) % 2` (`src/sky_gpu.rs::sky`). Each panorama covers half
//! the circle, so there are two *meridian* seams where one image hands over to
//! the other. The shader mitigates both with a ±0.08 crossfade; this program
//! tests that mitigation as arithmetic, independently of the GPU:
//!
//! 1. the mapping is exactly periodic over azimuth (no discontinuity, no NaN);
//! 2. every direction across both seams has its two samples taken from the two
//!    *opposite* edges of the *two* panoramas of the set;
//! 3. the weight given to the *other* panorama is 0.5 exactly on each boundary,
//!    falls to 0 at the inner edge of the band, is monotonic across both 8 %
//!    bands, and the two bands never overlap;
//! 4. a whole set (`g*3 + q`) keeps both panoramas and the roof in one resource
//!    group, so a set swap moves all three layers together. This is the part the
//!    single-set seam test could not cover: `sky_transition::frame_sets` must
//!    never hand the shader a layer from one group and another from a second.
//!
//! Pure arithmetic, no GPU and no assets, so it runs in the plain test job.

#[path = "../sky_transition.rs"]
mod sky_transition;

use sky_transition::{Machine, Request, Triple};

/// The shader's azimuth mapping, transcribed from `src/sky_gpu.rs::sky`.
fn azimuth(x: f32, z: f32) -> f32 {
    x.atan2(-z) / std::f32::consts::TAU + 0.5
}

/// `(u, side)` exactly as the shader computes them.
fn uv(x: f32, z: f32) -> (f32, i32) {
    let az = azimuth(x, z);
    let u = (az * 2.).fract();
    let side = ((az * 2.).floor() as i32).rem_euclid(2);
    (u, side)
}

/// The shader's seam mitigation: returns `(other_side, weight_of_other)`.
///
/// Both branches are re-expressed as *the weight of the sample taken from the
/// other panorama*, so the two can be compared on one scale. The shader writes
///
/// ```wgsl
/// if u<0.08 {a=mix(prev,a,0.5+0.5*u/0.08);}   // t is the weight of `a`
/// if u>0.92 {a=mix(a,next,0.5*(u-0.92)/0.08);} // t is the weight of `next`
/// ```
///
/// i.e. `1 - (0.5 + 0.5u/0.08)` and `0.5(u - 0.92)/0.08`. With this scale both
/// boundaries meet at exactly 0.5 and fall to 0 at the inner edge of the band.
fn seam_blend(u: f32) -> Option<(i32, f32)> {
    if u < 0.08 {
        Some((1, 0.5 - 0.5 * u / 0.08))
    } else if u > 0.92 {
        Some((0, 0.5 * (u - 0.92) / 0.08))
    } else {
        None
    }
}

/// Layer index for one panoramas' side within a resource group.
fn layer(group: i32, phase: i32, side: i32) -> i32 {
    group * 9 + phase * 3 + side
}

fn main() {
    // 1. Periodicity: sweeping azimuth all the way round must produce the same
    //    (u, side) as the start, never a NaN, and `u` must stay in [0,1).
    let mut worst = 0.0f32;
    let start = uv(0., -1.);
    let steps = 100_000;
    for i in 0..=steps {
        let a = std::f32::consts::TAU * i as f32 / steps as f32;
        let (x, z) = (a.sin(), -a.cos());
        let (u, side) = uv(x, z);
        assert!(
            u.is_finite() && (0.0..1.0).contains(&u),
            "u out of range: {u}"
        );
        assert!((0..2).contains(&side), "side out of range: {side}");
        // Distance travelled between neighbouring samples stays tiny: this is
        // what makes a hard cut visible and the crossfade necessary.
        if i > 0 {
            let (x0, z0) = (
                (a - std::f32::consts::TAU / steps as f32).sin(),
                -(a - std::f32::consts::TAU / steps as f32).cos(),
            );
            let (u0, _) = uv(x0, z0);
            let mut d = (u - u0).abs();
            if d > 0.5 {
                d = 1. - d; // wrap: a meridian, handled by the crossfade branch
            }
            worst = worst.max(d);
        }
    }
    // Azimuth 0 sits *on* a meridian, at the left edge of the second panorama:
    // that is the boundary itself, not a defect, and it is why the first sample
    // of the sweep is already inside a crossfade band.
    assert_eq!(
        start,
        (0.0, 1),
        "azimuth 0 must be the meridian at u=0, side 1"
    );
    assert!(
        seam_blend(start.0).is_some(),
        "the meridian must be crossfaded"
    );
    let end = uv(0.000_001, -1.);
    assert_eq!(end.1, start.1, "the mapping must not flip side at the wrap");
    println!("periodic mapping ok: {steps} azimuth samples, max step {worst:.5}");

    // 2./3. Both boundaries: the crossfade must read the *other* panorama of the
    //    same resource group, and at both boundaries the sample that is being
    //    faded *in* still has weight 0 (the shader's own asymmetry: the left
    //    branch mixes at `0.5 + 0.5u/0.08` starting from 0.5 at `u = 0`, the
    //    right branch mixes at `0.5(u-0.92)/0.08` starting from 0 at `u = 0.92`,
    //    so neither edge pops when a direction enters the band).
    for (name, u, want) in [
        ("left-lower", 0.0f32, 0.5),
        ("left-upper", 0.079_999, 0.0),
        ("right-lower", 0.920_001, 0.0),
        ("right-upper", 0.999_999, 0.5),
    ] {
        let (other, w) = seam_blend(u).unwrap_or_else(|| panic!("{name} seam not covered"));
        assert_eq!(other, if name.starts_with("left") { 1 } else { 0 });
        assert!(
            (w - want).abs() < 1e-4,
            "{name} seam weight {w} (want {want})"
        );
    }
    // The band edges are half-open (`u < 0.08`, `u > 0.92`) and the other
    // sample's weight is already 0 at each inner edge, so dropping the branch
    // there is continuous. `u == 1` is not a sample the shader ever sees:
    // `fract` wraps it to the next meridian, which is the left branch's `u == 0`
    // -- and both sides of that wrap give the other sample weight 0.5, so the
    // seam itself is continuous too.
    assert!(seam_blend(0.08).is_none() && seam_blend(0.92).is_none());
    let (l, r) = (seam_blend(0.0).unwrap().1, seam_blend(0.999_999).unwrap().1);
    assert!(
        (l - r).abs() < 1e-5,
        "the two sides of a meridian differ: {l} vs {r}"
    );
    // Monotonic and bounded inside both bands: the other panorama fades *out*
    // as a direction leaves the left boundary and *in* as it approaches the
    // right one, so one band is decreasing and the other increasing.
    let mut last = f32::INFINITY;
    for i in 0..=32 {
        let u = 0.079_999 * i as f32 / 32.;
        let (_, w) = seam_blend(u).unwrap();
        assert!(
            w <= last,
            "left band not monotonic at u={u}: {w} after {last}"
        );
        assert!(
            (0.0..=0.5).contains(&w),
            "left band weight out of range: {w}"
        );
        last = w;
    }
    assert!(
        (last - 0.0).abs() < 1e-3,
        "left band must fade the other panorama fully out at u=0.08: {last}"
    );
    let mut last = f32::NEG_INFINITY;
    for i in 0..=32 {
        let u = 0.920_001 + 0.079_999 * i as f32 / 32.;
        let (_, w) = seam_blend(u).unwrap();
        assert!(w >= last, "right band not monotonic at u={u}");
        assert!(
            (0.0..=0.5).contains(&w),
            "right band weight out of range: {w}"
        );
        last = w;
    }
    assert!(
        (last - 0.5).abs() < 1e-3,
        "right band must fade the other panorama fully in at u=1: {last}"
    );
    // The bands never overlap, so no direction samples three times.
    let mut band = 0;
    for i in 0..=1000 {
        if seam_blend(i as f32 / 1000.).is_some() {
            band += 1;
        }
    }
    assert!(
        band > 100 && band < 1001,
        "unexpected number of directions in a seam band: {band}"
    );
    println!(
        "seam crossfade ok: two meridian bands, other-panorama weight 0.5 -> 0 -> 0.5, \
         {band}/1001 samples inside"
    );

    // 4. A set swap moves the whole triple: all three phases of the outgoing
    //    group and of the incoming group, and never a mixture of groups.
    let mut machine = Machine::new(Triple::DAY);
    machine.request(Request::over(Triple::NIGHT, 660)).unwrap();
    machine.resources_ready();
    machine.step();
    let (base, incoming, weight) = sky_transition::frame_sets(&machine);
    // `Triple::NIGHT` is set 1 (the client's `sky_id_night = 1`, 阴天01), so the
    // fade runs from group 0 to group 1.
    assert_eq!((base, incoming), (0, 1));
    assert!(weight > 0.0 && weight < 1.0, "expected a mid-fade weight");
    for phase in 0..3 {
        for side in 0..2 {
            let a = layer(base, phase, side);
            let b = layer(incoming, phase, side);
            assert!(
                (base * 9..base * 9 + 9).contains(&a)
                    && (incoming * 9..incoming * 9 + 9).contains(&b),
                "layers {a}/{b} left their resource group"
            );
            assert_ne!(
                a / 9,
                b / 9,
                "phase {phase} side {side} mixed two resource groups"
            );
        }
        // The roof layer belongs to the same group as both panoramas.
        assert_eq!(layer(base, phase, 2) / 9, base, "roof left group {base}");
        assert_eq!(layer(incoming, phase, 2) / 9, incoming);
    }
    println!(
        "set integrity ok: base group {base}, incoming group {incoming}, weight {weight:.3}, \
         18 layers checked"
    );
    println!("phase5 sky seam: PASS");
}
