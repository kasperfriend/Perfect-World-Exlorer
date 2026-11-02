# Known unsupported or incompletely reproduced client paths

Updated 2 November 2026 against code revision `a9c4b35`. This is an explicit
**nonempty** register, not a claim of an exhaustive asset census. Unknown paths
may still be discovered. Per-requirement evidence and remaining acceptance are
in the [76-point reassessment](Eight-phase-reassessment-2026-11-02.md).

## Known rendering/runtime gaps

| Path / behavior | Remaining limitation | Requirements |
|---|---|---|
| General GFX owner graphs and scene/model-event lights | Scene effects and hook-free ECM `EventType 100` roots now have a real owner lifecycle (region load/unload, one root per update, visibility/priority gating, destruction). Skeleton-hook (`HookName`) attachment, visual GFX playback (particles/sprites/trails), controller-108 load-thread precision and the native owner fade/stop/release phase remain open. | P1.01–03, P7.04–05 |
| Controller-108 LUT construction precision | `gfx_color_track::CONSTRUCTION` is an explicit `Precision::Extended` choice, not a recovered fact. The integer and both float tables differ on thousands of interior entries (`tools/analyze-color-track-construction.py`: 17,790 and 16,332 of 8,257,536), so the loading-thread FPU mode must be traced before this is called native. | P1.01 |
| Alpha-list depth state | `model_queue::ALPHA_DEPTH_WRITE = false` is an assumption: the retail z-write state for the alpha list was not recovered. It is isolated in one named constant so a trace can change it without touching call sites. | P1.02–03 |
| Model mesh-level transparency | The current SKI reader exposes no per-mesh transparency field, so the strict-positive transparency override is driven by the frame value the caller supplies. Its native source is unestablished. | P1.02 |
| Non-ordinary receiving classes and alpha/special model queues | Ordinary weighted/rigid opaque paths exist and now route authored blend pairs into a real alpha queue with retail insertion order. Morph/supple receiving classes, fixed-function alternatives and exceptional materials remain explicit errors rather than approximations. | P1.02–03, P7.01–03 |
| Complete regional material consumers | Native coefficient/visibility calculations exist; complete downstream material/celestial consumer coverage is not established. | P1.04–05 |
| Native terrain/grass lightmap residency | Startup metadata and distinct missing-night policies exist; native missing-day retained binding and dynamic load/unload remain open. | P1.06 |
| Static BMD format/material alternatives | Reader accepts the inspected model version and mesh v4/v5, not all versions. All stages, alpha sorting and client overrides are incomplete. | P1.07, P3.01–07 |
| Final postprocess/color/presentation acceptance | Bloom/presenter/sharpen/gamma implementations exist; final pass audit, upstream color coverage and Windows compositor acceptance remain open. | P1.08–09 |
| Complete fog variants | Grass/BMD/terrain refraction paths exist; complete transparent/special-material and underwater ordering coverage is not established. | P1.10, P5.06, P6.07 |
| Native terrain selection, LOD and seams | Current 4-grid distant mesh and boundary interpolation are explicitly approximate. | P2.01–02 |
| Terrain holes and visibility/special-surface flags | No native hole/flag rendering implementation established. | P2.07 |
| All terrain shaders and source samplers | Specular/fallback variants and every material projection/mip/filter case remain incomplete. | P2.03–06, P2.08 |
| Native scenery culling, depth and residency | Reversed depth and duplicate-draw suppression exist; original bias/order, visibility, eviction and bounded full-world residency remain open. | P3.02–03, P3.06–08, P8.02–03 |
| Complete SpeedTree materials/runtime | Approved geometry remains. Full branch/frond/leaf/billboard materials, original continuous LOD and runtime wind/animation are incomplete. | P4.01–04, P4.07 |
| Complete native grass placement/wind/streaming | RANMAR placement, fades and wind history exist; original timing/precision, patch visibility, density/LOD and full streaming remain open. | P4.05–07 |
| Native sky resources and celestial materials | Current panorama/blend schedule and opt-in discs are incomplete; regional sky texture sets, halos and original geometry/blending remain open. | P5.01–04, P5.07–09 |
| Weather/cloud/precipitation runtime | No implemented native weather visual runtime established. | P5.05, P5.08 |
| Water quality/scroll/alternate paths | Multiple renderers and clipped targets exist; every quality/mode, clear/depth state, overlap ordering, complete lighting/fog, shoreline RNG and efficient multi-height behavior remain open. | P6.01–08 |
| Complex model actions and unimplemented formats | Ordinary source clocks exist; complex combined actions/events/random scheduling, native LOD/visibility, morphs and additional formats remain open. | P7.01–02, P7.06 |
| General visible effects | Particles, sprites, trails and dependent animated textures are not a general integrated visual effect renderer. Light-element decoding alone does not implement them. | P7.04–07 |
| Complete map scenery and non-main-world coverage | Map navigation exists; every model/effect/material and all maps/instances are not established complete. | P2.08, P3.07, P8.01, P8.04 |
| Native audio and environmental sound | Zone music exists; original priority/randomization/crossfades, ambient sounds, remaining codecs and physical-device behavior remain open. | P8.05 |
| Every setting and Windows runtime acceptance | Persistence/launcher/CI executable exist; all launcher controls, secondary-pass behavior, DPI/device/recovery and fresh-PC launch require further acceptance. | P8.06–07, P8.09 runtime follow-up |

## Completion policy

P8.10's **tracking/policy deliverable** is implemented by maintaining this register;
that does **not** mean its entries are resolved. P8.01's exhaustive census and
resolution requirement remains incomplete. Do not claim 100% parity while this
register has open entries, while the census is incomplete, or while full-scope
acceptance gates remain open. Removing rows, renaming gaps, or passing compilation
cannot satisfy that condition. Close entries only with implemented behavior and
specific validation evidence.

This register records behavior-level gaps. Existing named asset/dependency failure
reporting in `world_models.rs` and client survey manifests remains necessary;
this document does not replace a complete per-asset failure census.
