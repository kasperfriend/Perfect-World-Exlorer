# Native Perfect World effects restoration — audit and blockers

Date: 7 November 2026  
Target: the repository's inspected Perfect World 1.4.2 client path  
Status: **BLOCKED — visual GFX effects are not yet restored.** This report is an audit checkpoint, not an implementation or a claim of parity.

## What the current viewer already does

- `src/scene.rs` parses ECWLD v12 effect placements, retaining the 60-byte placement record, root path, scale, position, direction/up, playback speed, validity mode, alpha and attenuation.
- `src/world_models_gpu.rs` registers ECWLD effect roots and ECM `EventType 100` GFX roots with the owner manager. It also has a first-level model-hook attachment path for attached roots.
- `src/gfx_owner.rs` advances owner lifetimes and publishes supported GFX light elements into the scene-light system. It accepts text roots beginning with `MOXT` and extracts the light-producing element path.
- The viewer classifies ECWLD effect placements for regional residency in `src/gpu.rs`, but that is not a visual effect draw pass.
- The celestial renderer already contains a sun disc, the sunshine strip and a 15-quad flare chain. Bloom/glow post-processing is also a separate existing system. The weather path has rain/snow quads. These must be preserved and not mistaken for generic GFX coverage.

## Confirmed missing work in the GFX path

The current code itself marks binary GFX roots as unsupported and documents the following missing visual graph:

1. Decode the binary GFX root/container format found in the retail archive, without misclassifying valid roots as corrupt.
2. Parse and evaluate child/relative hook targets and parent binding (`ChildHook`, `RelHook`, `BindParent`), including the static `ECMHook` hanger path.
3. Render the actual particle, sprite, strip/beam, trail and animated-texture nodes.
4. Reproduce authored start/stop, repeat, delay, play-speed, time-span, probability/selection, visibility, alpha and attenuation/fade behaviour where present.
5. Preserve the root's world placement or live skeleton/weapon hook transform and propagate it through every child node.
6. Integrate the result into the real renderer with the correct material/blend/depth/sort state and the appropriate main, reflection and refraction/underwater pass rules.
7. Validate the complete set of real world placements and ECM model events. A light publication test, parser test, synthetic quad or successful shader compile is not a substitute for seeing the native assets in-world.

The repository's `Findings/Unsupported-client-paths.md` already records that 2,275 effect placements were identified, while the complete visual GFX graph remains missing. Its existing owner/light implementation is useful groundwork, not proof that waterfalls, torches or weapon effects are drawn.

## Secondary Dropbox inventory checked

The Secondary account's root folder listing completed with `has_more = false`. It contains nine files and no child folders:

| File | Size |
| --- | ---: |
| `gfx.pck` | 97,723,094 bytes |
| `shaders.pck` | 4,391,690 bytes |
| `litmodels.pck` | 397,425,471 bytes |
| `grasses.pck` | 12,141,665 bytes |
| `facedata.pck` | 25,727,486 bytes |
| `configs.pck` | 588,068 bytes |
| `ifc22.dll` | 196,608 bytes |
| `speedtreert.dll` | 450,560 bytes |
| `zlibwapi.dll` | 72,704 bytes |

This is not the complete client data set required to recover and validate the whole graph. In particular, the listing has no `textures.pck`, `models.pck`, `element/maps/world/world.ecwld`, `world.ecbsd`, referenced BSData `.dat` files, or `sfx.pck`. The missing model and texture dependencies matter for ECM/weapon attachments and for rendering GFX nodes that reference client textures. The world files are also needed to enumerate and validate the actual scene placements.

The Dropbox text-extraction endpoint cannot return these binary PCK archives: the 4.39 MB shader archive exceeded its connector response limit, and the 97.7 MB GFX archive exceeded the 5 MiB file-extraction limit. Temporary download URLs were issued, but this execution environment could not resolve the Dropbox download host. Therefore no archive entries were decoded in this run, and no effect assets are claimed as recovered.

## Inputs required to unblock implementation

Provide a matching 1.4.2 client asset set in the current conversation as an attached ZIP (or otherwise make the binary files directly available to the build workspace), containing at minimum:

- `element/gfx.pck`
- `element/textures.pck`
- `element/models.pck` and `element/litmodels.pck`
- `element/shaders.pck`
- `element/maps/world/world.ecwld` and `world.ecbsd`
- all BSData `.dat` files referenced by the world records
- the relevant element/client metadata needed to resolve effect names and animation dependencies

Keep these proprietary client archives out of Git. They are test inputs only; the viewer should continue loading them from the user's installed client.

## Verification gate (not yet run)

When the full input set is available, the implementation must not be called complete until all of the following are true:

- Real archive census: every root and dependency is classified as supported, intentionally irrelevant, or a named unsupported case; no silent skips.
- Real-asset parser tests: representative native roots cover binary/text formats, malformed/truncated inputs, nested graphs, hooks, timing and all supported node types.
- Production integration tests: scene placements and ECM events feed the visual renderer, not only the light publisher; transform and lifecycle tests cover unload/reload and attached-parent motion.
- GPU checks: authored blend/depth/alpha/texture animation and clipping are exercised with real decoded asset data.
- In-world review: waterfalls, fire/torches, weapon effects, environmental emitters and other discovered effect types are visibly present at their authored locations in the actual viewer. Check main view and relevant water/reflection passes; compare against the same client build.
- CI: format, all binary targets, viewer tests, new effect diagnostics, software-Vulkan checks and the Windows x64 release build pass.
- No proprietary PCKs, extracted client assets or derived client caches are committed.

## Current result

The branch is intentionally an audit checkpoint only. No production source files were changed, no real effect was added or marked as verified, and no build or in-world visual test was run in this environment. Do not merge this checkpoint as an effects implementation.
