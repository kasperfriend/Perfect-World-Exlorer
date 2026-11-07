# Native Perfect World effects restoration — asset audit and implementation gate

Date: 7 November 2026  
Target: the repository's inspected Perfect World 1.4.2 client path  
Status: **BLOCKED — source archives are present in Secondary Dropbox, but their binary contents are not readable by this execution workspace.** This report is not an implementation or a claim of visual parity.

## Existing viewer work relevant to effects

- `src/scene.rs` parses ECWLD v12 effect placements and retains the 60-byte native placement record: root path, scale, position, direction/up, play speed, validity mode, alpha and attenuation.
- `src/world_models_gpu.rs` registers ECWLD effect roots and ECM `EventType 100` GFX roots with the owner manager. Its current hook-binding path can follow a first-level model skeleton hook.
- `src/gfx_owner.rs` advances owner lifetimes and publishes supported GFX light elements into the scene-light system. It accepts text roots beginning with `MOXT`; this is a light-producer implementation, not a visual particle renderer.
- `src/gfx_decal.rs` has a narrow parser/evaluator for static, single-keypoint 3D GFX element 100 decals. Its unit test names real weapon-effect roots (dual axes, dual swords and coloured sword glows) when a local `google_drive/pw142/element/gfx.pck` test corpus is available.
- `src/gfx_decal_gpu.rs` has a GPU receiver for those static quads, but it is not connected to the production world-effects owner/render path.
- Sun discs, sunshine rays, a 15-quad celestial flare chain, bloom, and basic rain/snow quads already exist in other renderer systems. These are separate implementations and do not establish full GFX graph support.

## Confirmed missing GFX work

The repository's `Findings/Unsupported-client-paths.md` identifies 2,275 world effect placements and documents these unimplemented paths:

1. Decode the binary GFX root/container format used by retail assets. Current roots that are not text `MOXT` are classified as unsupported.
2. Recover all element families, including actual particles, sprites, strips/beams, trails and animated-texture controllers.
3. Resolve recursive child/relative hook relationships (`ChildHook`, `RelHook`, `BindParent`) and static `ECMHook` hanger objects.
4. Reproduce authored timing, repeat/delay, play speed, time span, probability/selection, stop/fade, alpha and attenuation behavior.
5. Preserve scene transforms and live model/weapon skeleton-hook transforms throughout child graphs.
6. Integrate visual effects into the actual renderer with recovered blend/depth/sort/material behavior and the correct main, reflection, refraction and underwater pass visibility.
7. Census and validate every real world placement and relevant ECM model event. A light-only owner test, static decal fixture, synthetic quad, or shader compile is not in-world visual verification.

## Updated Secondary Dropbox inventory

The Secondary account's root listing was re-read after the user's uploads. Pagination is complete (`has_more = false`), with 12 files and no child folders:

| File | Size |
| --- | ---: |
| `models.pck` | 888,028,116 bytes |
| `litmodels.pck` | 397,425,471 bytes |
| `maps.zip` | 493,486,753 bytes |
| `textures.pck` | 117,047,648 bytes |
| `gfx.pck` | 97,723,094 bytes |
| `grasses.pck` | 12,141,665 bytes |
| `facedata.pck` | 25,727,486 bytes |
| `shaders.pck` | 4,391,690 bytes |
| `configs.pck` | 588,068 bytes |
| `speedtreert.dll` | 450,560 bytes |
| `ifc22.dll` | 196,608 bytes |
| `zlibwapi.dll` | 72,704 bytes |

This corrects the earlier checkpoint: `models.pck` and `textures.pck` are present, and `maps.zip` is present. I am not claiming that the world files inside `maps.zip` have been verified; its bytes have not been extracted. Likewise, the presence of the archives does not yet prove they are the same build as the repository's inspected corpus.

The repository's `Findings/Models-pck-census.md` documents a `models.pck` corpus of the same byte size (888,028,116 bytes) with 40,753 members, including ECM, SMD, BON, SKI and STCK records. That is useful existing research, but the newly listed Dropbox file has not been read or hash-compared in this execution.

## Why implementation cannot yet use those uploads

The Dropbox connector can list these files, read their metadata and mint temporary download URLs. Its text extraction endpoint is capped at 5 MiB and does not provide the raw bytes of these binary PCK/ZIP archives. A direct-download URL was issued for the archives, but the execution environment cannot reach the Dropbox download host. The container's ordinary network lookup also fails, so the files cannot be fetched into the build workspace. Opening a temporary URL through the web reader does not make its binary payload available.

This is a byte-access limitation, not a claim that the user failed to upload the files. No PCK entries have been decoded in this run. Do not infer archive content from filenames, file sizes or the existing census.

## Remaining input-access requirement

No more Dropbox inventory is needed. The next step requires the binary bytes to be made available to a workspace that can actually read them, for example as conversation attachments or through an accessible local/build-runner path. At minimum the implementation needs the actual `gfx.pck`, `models.pck`, `textures.pck`, `litmodels.pck`, `shaders.pck`, and the extracted contents of `maps.zip` (including `world.ecwld`, `world.ecbsd` and the referenced BSData `.dat` files if they are in that archive). The repository already contains prior client executable disassembly for some subsystems; further executable recovery may still be needed for undocumented GFX node families.

Keep proprietary archives, extracted client assets and derived caches out of Git. These are test inputs only; the viewer should continue reading the user's installed client at runtime.

## Verification gate

The implementation is not complete until all of the following have evidence:

- Real archive census: every GFX root and dependency is supported, intentionally irrelevant, or listed as a named unsupported case; no silent skips.
- Real-asset parser tests for every recovered root/container and element family, including malformed/truncated records, nested graphs, hook chains, timing and animation.
- Production integration: ECWLD placements and ECM/weapon events feed the visual renderer, not just the light publisher; owner creation, movement, time gating, unload/reload and hook-parent motion are tested.
- GPU checks using real decoded assets for texture animation, authored blend/depth/alpha/sort behavior, and clipping.
- In-world verification in the actual viewer that waterfalls, fire/torches, weapon effects, environmental emitters and every other discovered type appear at their correct authored locations; check relevant water/reflection passes against the same client build.
- CI passes formatting, all binary targets, viewer tests, new effect diagnostics, software-Vulkan checks and the Windows x64 release build.
- No proprietary PCKs, extracted client assets or derived caches are committed.

## Current result

The branch remains an audit checkpoint. No production source has been changed in this branch, no real visual effect has been added or marked verified, and no build or in-world test has been run. The user has asked to test only after implementation is complete; this checkpoint is **not** a test-ready build and must not be merged.
