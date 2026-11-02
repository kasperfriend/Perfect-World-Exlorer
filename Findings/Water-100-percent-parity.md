> **Current status correction — 2 November 2026:** Use the [full eight-phase reassessment](Eight-phase-reassessment-2026-11-02.md) and [unsupported-path register](Unsupported-client-paths.md) for current decisions. The material below preserves historical milestones and may contain superseded absence claims. Old percentages are not current completion scores. Ordinary animated world models/scene-light submission, recovered postprocessing/gamma, per-material fog and clipped water/edge passes now exist; none of that establishes full phase parity.

# Water 100% parity

Acceptance: port the supplied PW 1.4.2 client's water behavior using its executable, shaders and real assets, across the world, above and below the surface. Preserve approved trees. The aim is a Windows executable and matching source, with visual comparison against the live game.

1. **Inventory:** Recover all water modes, asset formats, texture/filter settings, pass order, target formats/dimensions and visibility rules from the client. Verify against actual data, without assuming unavailable assets.
2. **Surface and reflections:** Port real geometry/masks, time-dependent normal textures, distortion, Fresnel, color space, clipping and sky/world reflections for every visible elevation.
3. **Refraction and edges:** Port per-plane refraction and underwater fog, actual shoreline edge mask target/pass, depth/occlusion and correct compositing across modes.
4. **Authored shoreline waves:** Load all versions, create original geometry, interpolate authored day/night colors, load original mipmapped texture, reproduce the three animated layers, render states and distance fading.
5. **Underwater:** Recover and implement below-water rendering, refraction, fog and postprocessing/transitions, including entry/exit and changing regions.
6. **World integration and completion:** Cover all world water records and quality settings; ensure no regressions to trees, scenery or controls. Verify code, GPU behavior and Windows build against recovered client evidence, then provide the executable and matching source. Live-game comparison is the user's task.

## Open issues
The edge mask is currently a terrain-depth approximation, underwater surface/refraction paths are incomplete, some water mode/target semantics remain unidentified, and shoreline noise/global RNG phase and complete client asset coverage have not been proven. The current build is experimental and these issues remain open.

### Latest edge-pass implementation (not yet parity-certified)
- The client disassembly places `0x7ccfb0` after the refraction scene and before the water surface; its state wrappers disable RGB writes, blending, lighting and fog. The viewer now rasterizes a terrain-height ramp into refraction alpha, preserving RGB and reusing scene depth, then samples that alpha at undisplaced projected coordinates for the expensive-water edge term.
- Both UNORM and sRGB offscreen GPU checks verify alpha ramp, preservation of refraction RGB, and rejection behind nearer depth. The old terrain-depth reconstruction remains the non-reflection fallback.
- **Still open:** prove the client's exact terrain-area selection, depth/write and clear-state behavior and geometry/culling against the disassembly; resolve low-quality water and underwater pass/transitions, RNG phase, integrated runtime and remaining acceptance gates. This is not a claim of 100% parity.

### Below-surface continuation
- Recovered the distinct `waterrefract.txt` path and added an above-plane clipped air-fogged refraction scene with an unmirrored sky; synthetic GPU checks cover steep and grazing below-water angles on UNORM and sRGB.
- Matched strict area boundary rejection and surveyed all 107 records: their center Y matches surface height, while 18 bounding-box pairs overlap. Client first-match ordering is still unknown.
- Open: scroll and cheap quality shaders, original target size/clear/depth details, register precision, below-surface fog, and original overlap/load order. Live appearance and final integrated Windows behavior remain unverified. No 100% claim.

### Area-class and underwater texture continuation
- Preserved the authored area-class flag and separated batches. Three of 107 real areas use the client's alternate fixed-function material; a provisional implementation was added in the later entry below.
- Ported quantized 256² underwater bump-map arithmetic with bilinear sampling. A gradient GPU readback agrees with the reference calculation.
- Full parity is **not complete**. In particular, alternate area material, quality/scroll modes, original exact pass states and target sampling remain open.

### Alternate-area material implementation
The renderer now routes flagged areas to a separate depth-read-only encoded-blend material and loads the client's 30 L-water texture frames and mips. Synthetic GPU tests cover its fixed-function color arithmetic and frame change in both target formats. This is **not complete parity**: dynamic water light factor, fog, quality/variant modes, ordering and real-texture validation are still open; the client PCK exceeds the connector's 100 MiB download limit, though the application can read it from the installed client.
