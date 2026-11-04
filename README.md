# Perfect World Explorer

An experimental, GPU-based, free-flight viewer for the **main world** of a Perfect World 1.4.2 client. It reads world data from a client you provide; it is not a game client, editor, server, or gameplay implementation.

> **Work in progress:** visual parity is not complete. Water, lighting, sky, vegetation and other systems contain approximations; ordinary animated world models now render with scene lights in main and clipped water passes, but complete model/material coverage and general visual effects remain unfinished. No complete side-by-side game comparison has established parity. See [Findings](Findings/README.md) for evidence, caveats and open work.

See the [full 76-point plan reassessment](Findings/Eight-phase-reassessment-2026-11-02.md) for implemented portions, remaining gates and validation limits. No phase is fully complete.

## What works

- Full-world terrain from client terrain files, with native-resolution materials, masks and day/night lightmaps; nearby detail streams as you fly.
- Static BMD buildings, rocks and scenery; optional whole-world object loading.
- Ordinary EC world models with retained skeleton/animation/material/texture data and scene-light assignment; complex actions, alpha/special queues and general effects remain incomplete.
- Client SpeedTree geometry through the client's 32-bit runtime, with sampled leaf motion and approximate LOD; locally streamed grass from client placements and meshes.
- Experimental animated water, per-height reflections and refraction, shoreline waves, underwater effects, environmental fog, sky, time-of-day control and optional bloom.
- Searchable teleport destinations, original and rendered world maps, a minimap, zone notices, Windows zone music and experimental stereo zone ambience.
- Windows launcher with remembered client folder and settings, and in-app Graphics, World, Sound and Interface controls. Settings → Graphics → **Resolution scale** renders the 3D world at 10–200% of the window size (default 100%) and upscales it, while the UI stays at native resolution; lower values trade sharpness for speed, higher values supersample.

Features are limited to formats and data encountered in one inspected build; other client variants may behave differently. This is not affiliated with or endorsed by the game or its publishers.

## Requirements

- 64-bit Windows 10 or 11, a compatible GPU with current drivers and a trusted, extracted Perfect World client with its data packages and 32-bit SpeedTree runtime. Game assets and the client are **not included**.
- Considerable memory and VRAM. Startup reads the terrain and may take time. Whole-world static scenery for the inspected client uses approximately **5.4 GiB of texture pages alone**, before geometry, terrain and other GPU resources. Use nearby streaming and the launcher's lower-memory terrain-mask option if needed.
- The launcher recognizes one particular 1.4.2 executable fingerprint. Other patched or regional clients are not guaranteed to work and may be marked unrecognized.

## Run the Windows executable

1. Download the Windows executable from the project's release (or the separately supplied `Windows` folder).
2. Run it and choose **Select Game Folder**. Select the extracted client root or its `element` directory.
3. Click **Load World**. The first run prepares tree geometry using the client runtime; wait for loading to finish.
4. The menu starts open. Choose Settings or close the menu to fly.

**Security:** The application executes the 32-bit `speedtreert.dll` from the selected client through an embedded helper. Only use a client you trust. Settings and the selected path are stored under `%LOCALAPPDATA%\PerfectWorldViewer`; derived tree geometry is cached in its `tree-cache` subfolder. The installation itself is not modified. Do not upload client data or the derived cache.

### Controls

| Control | Action |
| --- | --- |
| Left-click | Capture mouse for looking |
| Escape | Release mouse / open menu; return from Settings |
| W/A/S/D | Fly forward/left/back/right |
| E or Space / Q or Left Ctrl | Ascend / descend |
| Shift | Faster movement |
| Arrow keys | Look |
| M | Open world map |

Use **Escape → Teleport List** to search destinations. The world map offers original client tiles or a top-down render of the currently loaded world, mouse-wheel zoom, and optional click-to-teleport. Whole-world static objects need to finish loading to appear in the rendered map.

## Build from source

Install stable Rust and Cargo from [rustup](https://rustup.rs/), and a toolchain suitable for the target platform. From the repository root:

```sh
cargo build --release --bin perfect-world-explorer
cargo test --bin perfect-world-explorer
cargo check --bins
```

Dependency-free Phase 1 core checks can also be run with `tools/check-phase1-core.sh` (requires `rustc`; optionally set `RUSTC`). This checks CPU references only, not the application or shaders. For the production whole-frame gamma pass, run `cargo run --bin display-gamma-gpu-check` on a machine with a working GPU adapter. Client gamma is fullscreen-only and applies after world and UI composition; windowed mode does not apply the display ramp.

On Windows, use a compatible Rust Windows toolchain and the required compiler/linker (for MSVC, install the Visual Studio C++ build tools). A GNU cross-build from Linux requires the Rust target and MinGW linker:

```sh
rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu --bin perfect-world-explorer
```

The single executable embeds `tools/speedtree/speedtree-bake.exe`, a 32-bit helper built from `tools/speedtree/bake.cpp`. It invokes the **user's** runtime DLL; the DLL is not bundled. The helper binary is retained in the repository because the Rust build uses `include_bytes!`; Windows PR and release workflows rebuild it from `bake.cpp` before compiling the viewer. For local builds after changing the bridge source, rebuild it with a 32-bit MinGW C++ compiler, e.g. `i686-w64-mingw32-g++ -static -O2 -municode tools/speedtree/bake.cpp -o tools/speedtree/speedtree-bake.exe`. Compiler versions and flags may produce a different binary. The bridge also writes `tree-alpha-refs.csv` into its prepared cache directory, containing the native branch/frond alpha references for each of the 59 tree types and three LODs. `tools/launcher-art.jpg` is embedded at build time.

The executable distributed alongside this source was rebuilt from this packaged snapshot. Native Windows/physical-GPU verification of the latest changes and game-to-viewer image comparison remain outstanding.

## Development and verification

- `src/` contains parsers, scene loading, GPU rendering, UI and audio; `src/bin/` contains diagnostic and survey programs. These are development tools, not required to run the viewer.
- Example: `cargo run --release --bin pck-inspect -- /path/to/element/shaders.pck` lists archive entries. Input client files are required.
- `cargo run --bin water-record-survey -- <directory>` expects `world.ecwld`, `world.ecbsd` and referenced BSData `.dat` files in that directory. Set `PW_WATER_SAMPLE_69` to a local `69.dat` path to enable optional real-data water tests. Do not commit these files.
- GPU diagnostics include `water-pass-gpu-check`, `simple-water-gpu-check` and `post-gpu-check`. They require a working graphics adapter; synthetic tests do not prove in-game visual parity.
- `python tools/audio-census.py /path/to/element/sfx.pck --ffmpeg /path/to/ffmpeg` surveys archive audio and independently decodes it; this is not a test of the Windows playback backend. On Windows, `cargo run --release --bin audio-corpus-check -- <element directory>` instead reads and decodes packed SFX and loose music with rodio; it does not test audible output or detect every possible mid-stream decoding error.
- `cargo run --bin scene-sound-check -- /path/to/element` surveys world sound placements and checks their paths against `sfx.pck`.
- Some helper binaries and research scripts assume specific test data layouts. Consult their source and [Findings](Findings/README.md) before use.

## Known limitations

The water render target details, simple-water lighting/fog, quality modes, wave timing and underwater integration remain incomplete. Terrain, sky, grass placement/wind, SpeedTree LOD/wind, regional lighting and fog are approximations. Animated EC models and effects, full point-light integration and gameplay are missing. Whole-world memory use can be high; the map shows loaded content rather than all possible assets. Audio is Windows-only: zone ambience selection and day/night switching are approximations, and positional, weather, combat and other gameplay effects are not implemented. Full decoder coverage and physical-device playback remain unverified. Rendering and timing vary with client build, graphics driver and hardware. See the [roadmap](Findings/Roadmap.md), [parity assessment](Findings/Parity-Assessment.md), and [water plan](Findings/Water-100-percent-parity.md).

## Repository layout

```text
Cargo.toml, Cargo.lock   Rust package and locked dependencies
src/                     viewer, parsers, GPU code and diagnostics
tools/speedtree/         bridge source, probes and embedded helper
tools/launcher-art.jpg   embedded launcher image
Findings/                research, evidence, assessment and roadmap
LICENSE                  project license
```

## License, attribution and publishing

The project is marked **GPL-2.0-or-later**; see [LICENSE](LICENSE). BMD and terrain format research was informed by the GPL-2.0-or-later [GameForge static importer](https://github.com/shaddowdaniel/GameForge-3D-Studio-/blob/0a7fae8fcba0df7b0daa988db8696a0feaf49f7b/source/blender/io/angelica/formats/angelica_static.cc) and [terrain importer](https://github.com/shaddowdaniel/GameForge-3D-Studio-/blob/0a7fae8fcba0df7b0daa988db8696a0feaf49f7b/source/blender/io/angelica/formats/angelica_terrain.cc), copyright 2026 GameForge 3D Studio Authors. Leaf-card methods were informed by the GPL-2.0 [mt2-world-baker](https://github.com/moleh1307/mt2-world-baker); ECWLD layout research used the [RaGEZONE discussion](https://forum.ragezone.com/threads/map-editor.1252114/). Dependency licenses also apply.

**Before public distribution**, verify that you have the rights to publish the launcher artwork and all included code/binaries, and review third-party dependencies and applicable game terms. The provenance and redistribution rights of `tools/launcher-art.jpg` have not been established in this package; replace it with artwork you have rights to if necessary. Do not redistribute the proprietary game client, its DLLs, archives, extracted assets, or derived caches. The `.gitignore` reduces accidental commits but is not a substitute for reviewing staged files.
