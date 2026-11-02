# Known unsupported or incompletely reproduced client paths

Updated 2 November 2026 against code revision `a9c4b35`. This is an explicit
**nonempty** register, not a claim of an exhaustive asset census. Unknown paths
may still be discovered. Per-requirement evidence and remaining acceptance are
in the [76-point reassessment](Eight-phase-reassessment-2026-11-02.md).

## Known rendering/runtime gaps

| Path / behavior | Remaining limitation | Requirements |
|---|---|---|
| Visual GFX graph and multi-hook chains | The light-producing owner graph is complete for P1.01: scene effects, hook-free ECM `EventType 100` roots and `HookName`-attached roots have a real lifecycle (region load/unload, one root per update, visibility/priority gating, destruction) and attached roots follow the posed skeleton hook every frame (`0x7b25e6` bone × hook, `0x7b2543` model × hook, `0x6e1370` authored hook-local frame). Still open, and owned by the visual-effect graph rather than by light selection: `ChildHook`/`RelHook`/`BindParent` target selection, static `ECMHook` hanger objects (`0x6f89c0`), particle/sprite/trail playback and the authored fade/stop phase of the drawn effect. | P7.04–05 |
| Manager light-priority limit value | The gate and its state are traced and implemented (`0x70ba3c..0x70ba44`: `[element+0xdc] > [manager+0x134]` suppresses publication only, the element keeps progressing and returns alive; the limit is manager-wide, written by `0x40dc97`/`0x40e394`, and survives owner churn/region unload). What is not decoded is the *authored byte* that fills the limit: it arrives through a scene/environment record (`+0xce`, copied from `+0xca` at `0x59af52..0x59af76`) whose file format this reader does not parse, so `Manager::set_priority_limit` defaults to "no suppression" - the client at its highest light setting - instead of an invented threshold that would silently drop authored lights. | P1.01 |
| Controller-108 LUT construction precision | Resolved to PC24. The executable never creates a thread: no `_beginthreadex`/`_beginthread` IAT slot (`0x4d54ec`/`0x4d54f0`) is referenced anywhere in `.text` and `CreateThread` is not imported, so loading is frame-budgeted on the update thread (`0x5b1a00` returns after one root) and construction shares the sampling thread. That thread never changes the x87 control word - `_controlfp` (`0x4d5564`) has no call site and the only `fldcw`/`fnstcw` instructions in the image belong to static CRT math helpers (`0x7fdcb7`, `0x8001e5`, `0x80c878`, `0x863350`) that restore the caller's word - so the device's PC24 word applies. `0x6ec5dd` builds the table with x87 `fmul`/`fsub` only, which under PC24 rounds every intermediate to a 24-bit mantissa; production therefore builds the PC24 table (`gfx_color_track::CONSTRUCTION = Precision::Pc24`). `tools/analyze-color-track-construction.py` still records that the two modes differ (17,790 / 16,332 interior entries of 8,257,536), which bounds the exposure of any root loaded before the device exists (that path would use the startup extended word). | P1.01 |
| Alpha-list depth state | Recovered, no longer an assumption: the alpha/overlay submit path `0x436720` -> `0x436984` -> `0x7844d0` sets SRCBLEND 5 (`0x77a2c0`), DESTBLEND 6 (`0x77a2f0`), **ZWRITEENABLE off** (`0x77a230(0)`), CULLMODE 1 (`0x77a540(1)`) before `0x779ca0(mesh, 1)` submits at `0x78452b`; the tail restores ZWRITE 1 (`0x784678`), CULLMODE 3 (`0x784684`) and the material block (`0x779fc0(0,4)`). ZENABLE is never written in the bracket, so the alpha list keeps the depth **test** and only drops the write; `0x7a76a0`/`0x7a6c30` only override the blend pair and alpha test, and `0x7a7c60` never issues a ZWRITE call, so no blended mesh writes depth. `model_queue` implements this and a regression pins both directions. | P1.02–03 |
| Segmented `LightNum` light elements | Element 130 (`LightType: 1` point light with one keypoint) is executed. The other retail light family - the one whose fields are printed at `0x707b55` (`Segs: %d`, `LightNum: %d`, `WaveLen: %f`, `Interval: %d`, `Width: %f`, `Alpha: %f`, `Amplitude: %f`, `Pos1Enable`/`Pos2Enable`, `UseNormal`, `Normal`, `Filter`) - is not executed. A GFX root that declares such an element now fails explicitly with "unsupported GFX light element <id>", so an authored light can never be dropped silently; elements without light fields (particles/sprites/trails) are still skipped. | P1.01, P7.04 |
| Fixed-function lighting alternative | The same alpha/overlay bracket disables `D3DRS_LIGHTING` (`0x77a3c0` -> `0x77bd00(0x89, 0)` at `0x784510`, restored to 1 at `0x78466e`), i.e. that pass has no fixed-function per-vertex lighting. The receiving-model path itself is shader-based (`0x7924b0`/`0x7924c1` select the specular variant), so this reader keeps scene lighting on the alpha queue; whether `0x436720` ever draws receiving models through the fixed-function pipeline instead of the shader is not established, so the FFP alternative is recorded rather than implemented. | P1.02–03 |
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
