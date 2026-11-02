//! Complete pass/state plan for the recovered client postprocess chain.
//!
//! Native order recovered from the 1.4.2 executable (the full pass matrix is
//! in `Findings/Phase-1-Agent-3-report.md`):
//!
//! 1. `0x40fce0` captures the scene into the full-resolution A8R8G8B8 target.
//! 2. `0x40fd33` runs spacewarp (`0x410250`) while underwater and `0x40fd58`
//!    copies the quantized result back into the capture target.
//! 3. `0x40fdb0` stretches the capture into the quarter-size target and applies
//!    `fullglow1_14` on the horizontal axis.
//! 4. `0x40f053` applies `fullglow2_14`/`monofullglow_14` on the vertical axis
//!    and composites `lrp(original, blur, level) + power * blur²` (the mono
//!    variant uses `blur * original`).
//! 5. `0x40fd64..0x40fd9c` runs `fullsharpen_14` after glow when renderer flag
//!    +6 is set. The flag is independent of glow.
//! 6. The display ramp (whole-frame in this viewer, see `color_chain`) and
//!    exactly one sRGB output encode are presentation concerns.
//!
//! Only enabled stages may run: a disabled presenter freezes rather than
//! clearing, and an unused chain must not allocate targets or quantize the
//! frame. The retained screen-space depth fog is a diagnostic pass; production
//! per-material fog replaced it, so it must never force the chain on its own.

/// Caller inputs that decide the chain for one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Inputs {
    /// Client glow presenter enabled (modes 1/2/3/4/5).
    pub bloom: bool,
    /// Renderer flag +6, `fullsharpen_14`.
    pub sharpen: bool,
    /// Camera below an active water plane (spacewarp).
    pub underwater: bool,
    /// Diagnostic depth fog. Never requested by production shading.
    pub screen_fog: bool,
    /// Camera fog is active (atmospheric fog enabled, or submerged). The
    /// client only assembles the fog state in those cases, so the retained
    /// diagnostic pass must not invent a third state.
    pub fog_enabled: bool,
    /// Display ramp inside the scene chain (windowed approximation only).
    pub display_gamma: bool,
    /// The composite target is an sRGB format, which encodes on write.
    pub srgb_target: bool,
}

/// Stages in native execution order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Capture,
    ScreenFog,
    Warp,
    Downsample,
    BlurHorizontal,
    Composite,
    Sharpen,
    DisplayRamp,
    OutputEncode,
}

/// Every stage executes in exactly this relative order.
pub const ORDER: [Stage; 9] = [
    Stage::Capture,
    Stage::ScreenFog,
    Stage::Warp,
    Stage::Downsample,
    Stage::BlurHorizontal,
    Stage::Composite,
    Stage::Sharpen,
    Stage::DisplayRamp,
    Stage::OutputEncode,
];

/// Result of the enable/disable audit for one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Plan {
    pub capture: bool,
    pub fog: bool,
    pub warp: bool,
    pub glow: bool,
    pub sharpen: bool,
    pub display_gamma: bool,
    /// Exactly one output encode happens for an sRGB target and none for a
    /// UNORM target (the encoded bytes are then already final).
    pub output_encode: bool,
}

impl Plan {
    /// Stages that run, in native order.
    pub fn stages(&self) -> Vec<Stage> {
        let mut out = Vec::new();
        for stage in ORDER {
            let on = match stage {
                Stage::Capture | Stage::Composite => self.capture,
                Stage::ScreenFog => self.fog,
                Stage::Warp => self.warp,
                Stage::Downsample | Stage::BlurHorizontal => self.glow,
                Stage::Sharpen => self.sharpen,
                Stage::DisplayRamp => self.display_gamma,
                Stage::OutputEncode => self.output_encode,
            };
            if on {
                out.push(stage);
            }
        }
        out
    }

    /// True when any stage touches the frame. When false the viewer renders
    /// straight into the presentation target with no copy or quantization.
    pub fn active(&self) -> bool {
        self.capture || self.warp || self.glow || self.sharpen || self.display_gamma
    }
}

/// Resolve the complete plan. Every rule below is covered by a regression.
pub fn plan(input: Inputs) -> Plan {
    // The scene capture exists for any stage that reads the composed scene back
    // (`0x40fce0`). The diagnostic fog needs it too; nothing else does.
    let capture =
        input.bloom || input.sharpen || input.underwater || input.screen_fog || input.display_gamma;
    Plan {
        capture,
        fog: capture && input.screen_fog && input.fog_enabled,
        // Spacewarp is gated on the underwater state only (0x40fd33). It is a
        // distortion of the captured scene, not a glow-strength effect.
        warp: capture && input.underwater,
        // Glow requires the presenter to be enabled; zero strength still runs
        // the recovered passes because the composite is then an identity.
        glow: capture && input.bloom,
        sharpen: capture && input.sharpen,
        // The scene-chain ramp is the windowed approximation only. The
        // whole-frame presenter ramp owns retail fullscreen gamma, so the
        // caller must never request both (`color_chain::ramp_stages`).
        display_gamma: capture && input.display_gamma,
        // Exactly one output encode: only when the composite target is sRGB.
        output_encode: capture && input.srgb_target,
    }
}

/// Whether the auxiliary targets and the composite pass must be allocated for
/// real scene effects. `screen_fog` is deliberately excluded: it is a
/// diagnostic route that production never requests.
pub fn required(input: Inputs) -> bool {
    plan(Inputs {
        screen_fog: false,
        ..input
    })
    .active()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_inputs() -> Vec<Inputs> {
        let mut out = Vec::new();
        for bits in 0u32..128 {
            out.push(Inputs {
                bloom: bits & 1 != 0,
                sharpen: bits & 2 != 0,
                underwater: bits & 4 != 0,
                screen_fog: bits & 8 != 0,
                fog_enabled: bits & 16 != 0,
                display_gamma: bits & 32 != 0,
                srgb_target: bits & 64 != 0,
            });
        }
        out
    }

    #[test]
    fn nothing_enabled_is_a_true_no_op() {
        let input = Inputs::default();
        let resolved = plan(input);
        assert!(!resolved.active());
        assert!(!required(input));
        assert!(resolved.stages().is_empty());
        // A non-sRGB target cannot encode values that were never written.
        let plain = plan(Inputs {
            srgb_target: false,
            ..Inputs::default()
        });
        assert_eq!(plain, resolved);
    }

    #[test]
    fn screen_fog_never_forces_the_production_chain() {
        let diagnostic = Inputs {
            screen_fog: true,
            fog_enabled: true,
            srgb_target: true,
            ..Inputs::default()
        };
        // The diagnostic route still needs the capture, but it must not make
        // the production chain mandatory.
        assert!(plan(diagnostic).fog);
        assert!(plan(diagnostic).capture);
        assert!(!required(diagnostic));
        for input in all_inputs() {
            let without = Inputs {
                screen_fog: false,
                ..input
            };
            assert_eq!(required(without), plan(without).active());
            // Requesting only the diagnostic flag never allocates the chain.
            let diagnostic_only = Inputs {
                screen_fog: true,
                ..input
            };
            assert_eq!(required(diagnostic_only), required(without));
        }
    }

    #[test]
    fn screen_fog_requires_an_active_camera_fog_state() {
        let off = Inputs {
            screen_fog: true,
            fog_enabled: false,
            srgb_target: true,
            ..Inputs::default()
        };
        assert!(!plan(off).fog, "no fog state must skip the pass");
        let on = Inputs {
            fog_enabled: true,
            ..off
        };
        assert!(plan(on).fog);
        // Neither request resurrects the production chain.
        assert!(!required(off));
        assert!(!required(on));
        // Submersion alone does not bypass the caller's diagnostic gate: the
        // production caller still has to ask for the pass.
        let submerged = Inputs {
            underwater: true,
            ..off
        };
        assert!(!plan(submerged).fog);
        assert!(plan(Inputs {
            fog_enabled: true,
            ..submerged
        })
        .fog);
    }

    #[test]
    fn every_effect_independently_allocates_the_chain() {
        let cases = [
            (
                Inputs {
                    bloom: true,
                    ..Inputs::default()
                },
                Stage::Downsample,
            ),
            (
                Inputs {
                    sharpen: true,
                    ..Inputs::default()
                },
                Stage::Sharpen,
            ),
            (
                Inputs {
                    underwater: true,
                    ..Inputs::default()
                },
                Stage::Warp,
            ),
            (
                Inputs {
                    display_gamma: true,
                    ..Inputs::default()
                },
                Stage::DisplayRamp,
            ),
        ];
        for (input, stage) in cases {
            let resolved = plan(input);
            assert!(resolved.capture);
            assert!(resolved.stages().contains(&stage), "{input:?}");
            assert!(required(input));
        }
    }

    #[test]
    fn bloom_pairs_downsample_with_the_horizontal_blur() {
        let resolved = plan(Inputs {
            bloom: true,
            srgb_target: true,
            ..Inputs::default()
        });
        assert_eq!(
            resolved.stages(),
            vec![
                Stage::Capture,
                Stage::Downsample,
                Stage::BlurHorizontal,
                Stage::Composite,
                Stage::OutputEncode
            ]
        );
    }

    #[test]
    fn warp_precedes_glow_and_sharpen_follows_it() {
        let stages = plan(Inputs {
            bloom: true,
            sharpen: true,
            underwater: true,
            srgb_target: true,
            ..Inputs::default()
        })
        .stages();
        let index = |stage: Stage| stages.iter().position(|s| *s == stage).unwrap();
        assert_eq!(index(Stage::Capture), 0);
        assert!(index(Stage::Warp) < index(Stage::Downsample));
        assert!(index(Stage::BlurHorizontal) < index(Stage::Composite));
        assert!(index(Stage::Composite) < index(Stage::Sharpen));
        assert!(index(Stage::Sharpen) < index(Stage::OutputEncode));
    }

    #[test]
    fn sharpen_is_independent_of_bloom() {
        let sharpen_only = plan(Inputs {
            sharpen: true,
            ..Inputs::default()
        });
        assert!(sharpen_only.sharpen);
        assert!(!sharpen_only.glow);
        // A plain composite still runs so the sharpen pass reads a source that
        // is quantized exactly like the native encoded target.
        assert_eq!(
            sharpen_only.stages(),
            vec![Stage::Capture, Stage::Composite, Stage::Sharpen]
        );
        let glow_only = plan(Inputs {
            bloom: true,
            ..Inputs::default()
        });
        assert!(!glow_only.sharpen);
        assert!(!glow_only.stages().contains(&Stage::Sharpen));
    }

    #[test]
    fn warp_runs_without_glow_and_before_the_composite() {
        let warp_only = plan(Inputs {
            underwater: true,
            ..Inputs::default()
        });
        assert!(warp_only.warp);
        assert!(!warp_only.glow);
        assert_eq!(
            warp_only.stages(),
            vec![Stage::Capture, Stage::Warp, Stage::Composite]
        );
    }

    #[test]
    fn screen_fog_runs_before_warp_and_glow() {
        let stages = plan(Inputs {
            screen_fog: true,
            fog_enabled: true,
            underwater: true,
            bloom: true,
            ..Inputs::default()
        })
        .stages();
        let index = |stage: Stage| stages.iter().position(|s| *s == stage).unwrap();
        assert!(index(Stage::ScreenFog) < index(Stage::Warp));
        assert!(index(Stage::Warp) < index(Stage::Downsample));
    }

    #[test]
    fn output_encode_happens_exactly_once_and_only_for_srgb_targets() {
        for input in all_inputs() {
            let resolved = plan(input);
            let encodes = resolved
                .stages()
                .iter()
                .filter(|s| **s == Stage::OutputEncode)
                .count();
            if resolved.active() && input.srgb_target {
                assert_eq!(encodes, 1, "{input:?}");
            } else {
                assert_eq!(encodes, 0, "{input:?}");
            }
            let ramps = resolved
                .stages()
                .iter()
                .filter(|s| **s == Stage::DisplayRamp)
                .count();
            assert_eq!(ramps, usize::from(resolved.display_gamma));
        }
    }

    #[test]
    fn every_declared_stage_is_reachable_and_ordered_once() {
        for (index, stage) in ORDER.iter().enumerate() {
            assert!(!ORDER[..index].contains(stage));
            let reached = all_inputs()
                .into_iter()
                .any(|input| plan(input).stages().contains(stage));
            assert!(reached, "{stage:?} is unreachable");
        }
    }
}
