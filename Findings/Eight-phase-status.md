# Eight-phase implementation status

Updated 2 November 2026. The original requested requirements are retained below.
Unchecked means **partial or not implemented**, not absent by default. Checked test/preservation/package/policy tasks mean their stated deliverable exists, NOT that their parent phase has native parity.
Requirement-level acceptance checks: [Phase 1 completion checks](Phase-1-completion-checks.md). Existing partial
implementations are documented in the research logs; no phase is complete by inference.

Execution gate: **Phase 1 open**. See [Phase 1 audit](Phase-1-audit-2026-11-02.md).
No phase is fully complete. The [full 76-point reassessment](Eight-phase-reassessment-2026-11-02.md) records implemented portions, evidence and specific gaps for every requirement. The [unsupported-path register](Unsupported-client-paths.md) remains nonempty.


## Phase 1 — Complete the lighting and display pipeline

- [x] P1.01 — Trace the client’s complete scene-light selection path, including the static grid, dynamic candidates, priority rules, range checks, flags and day/night scaling.
- [x] P1.02 — Identify every object class that receives scene lights and reproduce its light assignment, attenuation, color, ambient and diffuse calculations.
- [x] P1.03 — Integrate those lights into the appropriate model shaders without adding them a second time to already-baked building or terrain lighting.
- [x] P1.04 — Finish the client’s regional ambient, directional and sun/moon lighting calculations, including region overlap priority, transitions and time-of-day behavior.
- [x] P1.05 — Connect the recovered sun-direction curve only where the client actually uses it for material lighting.
- [x] P1.06 — Complete terrain and grass lightmap selection, coordinates, blending, sampler states and shader arithmetic.
- [x] P1.07 — Finish baked-model day/night color interpolation, material flags, alpha behavior and any client-specific light overrides.
- [ ] P1.08 — Recover the remaining bloom constants, texture coordinates, render-target formats, effect transitions and pass order.
- [ ] P1.09 — Complete the gamma chain: client settings, fullscreen versus windowed behavior, color conversions, output encoding and interaction with postprocessing.
- [ ] P1.10 — Replace the approximate screen-space fog with the client’s applicable per-pass fog behavior, including regional transitions and underwater conditions.
- [x] P1.11 — Keep the approved tree appearance intact while tracing its actual lighting separately; replace the fallback only when the complete tree material path is available.
- [x] P1.12 — Add automated tests for light selection, transitions, shader arithmetic, color-space conversions and postprocess passes.

## Phase 2 — Complete terrain and ground materials

Nine work points (re-cut 2 November 2026; the Phase 2 scope is unchanged, only its
boundaries are now non-overlapping and independently verifiable). Execution order
and ownership: [Phase 2 plan and agent prompts](Phase-2-plan-and-agent-prompts.md).

- [ ] P2.01 — Reproduce terrain block and area residency and draw selection: which T2MK areas and height blocks are resident for a camera position, which are submitted, how the streaming window grows and evicts, and explicit failures for missing or malformed blocks.
- [ ] P2.02 — Reproduce terrain resolution levels and their transitions: the local full-resolution mesh, the distant coarse mesh, the client's own level thresholds and hysteresis, and the geometry and vertex path used at each level.
- [ ] P2.03 — Replace approximate terrain seam stitching with the client's own edge and neighbour rules, including block boundaries and boundaries between resolution levels.
- [ ] P2.04 — Complete every terrain shader variant and pass state: opaque and alpha surfaces, near and far vertex paths, stage setups, cull/depth/blend/alpha-test state, and the hardware fallback paths the client actually uses.
- [ ] P2.05 — Complete terrain lightmap selection and arithmetic: T2LM/T2LM1 pair choice, tile addressing and clamp/wrap behavior, day/night blend, encoded-space modulation, and how the result combines with regional light.
- [ ] P2.06 — Verify and implement every terrain material layer: layer axes and projections, ordering, per-area counts, mask formats and resolutions, and the world-to-texture mapping for each authored area.
- [ ] P2.07 — Match terrain texture sourcing and sampling: source dimensions, mip chain generation, addressing modes, anisotropic filtering, per-material sampler settings and every source format in the archive.
- [ ] P2.08 — Implement terrain specials and detail surfaces: holes and visibility flags, special surface cases found in client data, and maximum-quality road and city detail including Archosaur's high-resolution surfaces.
- [ ] P2.09 — Keep terrain consistent across views (main, reflection, clipped refraction, minimap and full-world map) with one snapshot per frame, and keep boundary, shader-variant, layer-selection and near/far LOD-continuity regressions running in CI.

## Phase 3 — Complete buildings, rocks and other static models

- [ ] P3.01 — Finish the BMD format and renderer: all versions, material fields, texture references, transforms and per-mesh flags.
- [ ] P3.02 — Trace and implement the client’s actual culling and two-sided-material rules rather than using a global approximation.
- [ ] P3.03 — Identify the real causes of building Z-fighting and reproduce the client’s depth, bias, ordering and overlapping-geometry behavior.
- [ ] P3.04 — Implement all static-model texture stages, alpha tests, transparency, material colors and baked-light combinations.
- [ ] P3.05 — Handle missing, shared and animated texture references using the client’s original lookup rules.
- [ ] P3.06 — Reproduce client model visibility, distance selection, LOD and regional loading.
- [ ] P3.07 — Complete reflection, fog, minimap and map rendering for every supported static-model material.
- [ ] P3.08 — Improve atlas allocation and streaming so full-resolution assets do not require all world textures to remain resident simultaneously.
- [ ] P3.09 — Add regression tests for model versions, culling, texture stages, depth conflicts and day/night baked lighting.

## Phase 4 — Complete SpeedTree and grass

- [ ] P4.01 — Recover and reproduce the client’s SpeedTree material and lighting paths for branches, fronds, leaves and billboards.
- [ ] P4.02 — Complete the original runtime’s geometry, LOD thresholds, transitions and billboard selection without reverting to camera-facing sprites where real geometry should be used.
- [ ] P4.03 — Replace the sampled leaf-motion approximation with the client’s actual runtime animation behavior where technically possible.
- [ ] P4.04 — Reproduce the client’s wind modes, timing, camera inputs and per-tree parameters; do not invent movement for components the client leaves static.
- [ ] P4.05 — Complete grass placement, terrain height alignment, density, type selection, patch visibility and distance LOD.
- [ ] P4.06 — Finish the client’s grass wind history, shader constants, fading, alpha tests and material behavior.
- [ ] P4.07 — Match vegetation texture filtering, mipmaps, cutout thresholds, lighting and reflection behavior.
- [x] P4.08 — Keep existing approved tree rendering available until the replacement path is complete.
- [x] P4.09 — Add tests for tree LOD boundaries, foliage animation, grass placement, wind, alpha coverage and lighting.

## Phase 5 — Complete sky, weather and atmospheric environment

- [ ] P5.01 — Reproduce the client’s sky geometry, texture layers, panorama mapping and seam handling.
- [ ] P5.02 — Implement the actual day/night sky blending and time-dependent texture selection.
- [ ] P5.03 — Complete sun, moon, halos and related celestial geometry, positioning, visibility and blending.
- [ ] P5.04 — Connect region-authored sky settings and transition behavior to the world environment.
- [ ] P5.05 — Implement client weather visuals and their timing, including clouds, precipitation and other atmospheric effects present in the supplied world.
- [ ] P5.06 — Complete regional fog, atmospheric colors and visibility behavior across normal, underwater and reflected views.
- [ ] P5.07 — Reproduce environment transitions when crossing region boundaries or changing the time slider.
- [ ] P5.08 — Ensure sky and weather behave correctly in reflections and do not shift incorrectly with camera altitude.
- [ ] P5.09 — Add tests for time boundaries, region transitions, celestial placement and sky seams.

## Phase 6 — Complete water and underwater rendering

- [ ] P6.01 — Trace the client’s water renderer and implement its original render targets, geometry, shaders and pass sequence.
- [ ] P6.02 — Reproduce water material selection, area masks, heights, shoreline behavior and visibility.
- [ ] P6.03 — Implement the original wave animation, normal maps, scrolling and time-dependent parameters.
- [ ] P6.04 — Complete reflection and refraction using the client’s camera transforms, clipping, depth handling and texture coordinates.
- [ ] P6.05 — Replace approximate reflection and refraction safeguards with the recovered client rules.
- [ ] P6.06 — Match water transparency, fresnel or equivalent material calculations, highlights, color and interaction with regional lighting.
- [ ] P6.07 — Complete underwater entry detection, distortion, fog, color and transition behavior in the correct order.
- [ ] P6.08 — Handle multiple visible water heights efficiently without losing their individual reflections.
- [x] P6.09 — Add tests for water masks, multiple heights, reflected cameras, depth ordering and underwater transitions.

## Phase 7 — Add animated world models and visual effects

- [ ] P7.01 — Finish parsing and rendering the client’s EC/animated world models, including their geometry, materials, skeletons and animation data.
- [ ] P7.02 — Implement their animation playback, timing, transforms, LOD and visibility.
- [ ] P7.03 — Connect the scene-light system from Phase 1 to the model classes confirmed to receive it.
- [ ] P7.04 — Implement world effects and their dependent resources: particles, sprites, trails, animated textures and other client effect types.
- [ ] P7.05 — Reproduce effect attachment points, timing, blending, depth behavior and region-based activation.
- [ ] P7.06 — Add any remaining world-object formats or specialized shaders discovered during the client-file census.
- [ ] P7.07 — Integrate animated models and effects into normal views, reflections and fog as the client does.
- [x] P7.08 — Keep gameplay systems out of scope: render world content without adding combat, quests, networking or player simulation.
- [x] P7.09 — Add tests for animation sampling, effect lifetimes, resource loading and material blending.

## Phase 8 — Full-world integration, optimization and delivery

- [ ] P8.01 — Complete a census of world files, object types, materials and shaders; resolve remaining unsupported or silently skipped assets.
- [ ] P8.02 — Implement client-equivalent visibility and streaming so the entire world remains explorable without loading every full-resolution texture into GPU memory at once.
- [ ] P8.03 — Optimize GPU memory, draw batching, culling, texture residency and background loading while retaining maximum-quality assets.
- [ ] P8.04 — Complete the realistic full-world map and minimap so their scenery, orientation and loading behavior remain consistent with the 3D world.
- [ ] P8.05 — Finish zone music and environmental audio selection, transitions, volume behavior and remaining client audio formats.
- [ ] P8.06 — Preserve and polish the launcher, remembered game-folder selection, loading screen, controls, settings, teleport list and map interaction.
- [ ] P8.07 — Ensure all settings are persistent, have clear defaults and apply consistently to the main view and relevant secondary passes.
- [x] P8.08 — Expand automated coverage across parsers, shaders, render passes, world streaming, navigation, audio and Windows packaging.
- [x] P8.09 — Produce the single Windows executable with the required embedded components, plus the complete source.
- [x] P8.10 — Maintain an explicit list of any unsupported client paths; 100% parity is reached only when that list is empty, not by assigning an estimated percentage.
