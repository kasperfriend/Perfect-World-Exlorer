# Native Perfect World effects restoration — asset audit and implementation gate

Date: 7 November 2026  
Target: the repository's inspected Perfect World 1.4.2 client path  
Status: **ASSET ACCESS PREPARED — the Secondary Dropbox folder is now confirmed public, view-only and download-enabled; a private GitHub Actions census workflow is in place but has not yet been run.** This report is not an implementation or a claim of visual parity.

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

## Binary access and recovery workflow

The user supplied a Dropbox folder shared link. Its current settings were verified through the Secondary Dropbox connection: `audience=public`, `access_level=view`, `allow_download=true`, target folder `PW`. The previous folder link used edit access; the current link is correctly view-only.

The execution sandbox still cannot retrieve raw bytes directly from Dropbox. To avoid exposing the public Dropbox link in Git, a manual GitHub Actions workflow has been added at `.github/workflows/effects-asset-recovery.yml`. It reads the link from the repository Actions secret `PW_SHARED_FOLDER_URL`, downloads the folder ZIP to an ephemeral runner, validates required archives, computes hashes, inventories PCK roots and ECM references, and inspects the `maps.zip` index. Only metadata/census reports are retained as a seven-day Actions artifact; proprietary source archives are not committed or uploaded as artifacts.

The workflow has not yet been run. The repository's GitHub connector does not expose the Actions secrets write API, so the user must add one repository secret named `PW_SHARED_FOLDER_URL` containing the current Dropbox folder link. Then manually run **Actions → Effects asset recovery and census → Run workflow** on branch `effects/restore-native-world-effects`. Do not paste the link into source files, issues, or workflow logs.

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


## Immediate next action and current state

- Added `tools/effects-archive-census.py`, a metadata-only scan that extracts GFX root headers/strings/resource-path candidates and ECM-to-GFX references from retail PCKs.
- Added `.github/workflows/effects-asset-recovery.yml` on the existing effects branch. It downloads and extracts the shared folder on a disposable GitHub runner; records archive hashes; attempts both retail and Angelica package censuses; and reports map/world index entries.
- The workflow is intentionally manual (plus an explicit trigger-file path), does not push client binaries into Git, and publishes only metadata reports with seven-day retention.
- **Not yet verified:** archive bytes, GFX member format, map members, census output, parser compatibility, production effect rendering, builds, GPU behavior, or in-world visuals.
- The workflow needs the GitHub Actions secret `PW_SHARED_FOLDER_URL` before it can run. Add the current view-only Dropbox folder URL at the repository Actions secrets page, then dispatch the workflow on this branch.
- Keep PR #31 draft and unmerged until implementation and real in-world verification are complete.
