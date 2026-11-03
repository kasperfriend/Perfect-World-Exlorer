# Agent 5-B — Region sky, weather and fog (P5.04–P5.06)

Date: 2026-11-03. Evidence uses the supplied Perfect World 1.4.2 `element` client set, not the separate Forsaken World folder. Client binaries and archives remain under ignored `google_drive/`.

## State table

| Point | State after this pass | Implemented / verified | Remaining |
|---|---|---|---|
| **P5.04 Region sky settings** | **Partial** | Fixed non-streamed regional selection so a containing local box supplies fog/colour while a containing default/global box independently supplies celestial direction/flags. The streamed path already uses native-like active-box ordering and independent local/default selection. Added overlap regressions. Verified that supplied configs and texture archives contain multiple sky IDs/sets. | The main-world v12 EC-BSD box record has no identified sky-set field, and no asset-to-main-world region mapping has been recovered. `SkyGpu` still loads the experimental `clear/evening/night2` set; do not treat AutoFamily editor IDs as proof of the main-world runtime mapping. The non-streamed overlapping-local tie order is stable EC-BSD export-ID order, not proven native load/hash order. |
| **P5.05 Weather visual runtime** | **Not implemented (visual runtime still missing)** | Supplied executable initializes native rain/snow objects and calls each object’s update vtable slot on a fixed 33 ms accumulator (`0x429370`, strict `accumulator > 33`). Added a tested `WeatherClock` matching that threshold, catch-up loop and DWORD wrap. PCKs contain rain/snow sprites, cloudy/rain sky sets and `textures/sky/cloud.tga`; world effects preserve a separate day/night eligibility field. | No renderer-owned cloud/precipitation visuals, regional weather selector or weather resource transitions. ECWLD/GFX effect roots do not establish a global rain/snow/cloud schedule; their valid-time selector is not proof of weather. No precipitation was invented. |
| **P5.06 Regional fog / visibility** | **Partial** | Existing regional day/night air and underwater fog, atmospheric colours, transition coefficients, camera underwater selection, reflected-view table fog, clipped-refraction fog and below-surface air fog remain wired. Added a regression asserting distinct normal/reflected/underwater/refraction/below-surface choices and that fog enable is independent of camera far clip. | Native material/weather fog variants and authoritative main-world visibility/far-distance policy are not fully recovered. Above-water fog remains an explicit opt-in; the app's far clip is separately controlled (default 16,000; adjustable 2,000–24,000; whole-world mode raises it to at least 32,000). Those app values are not claims about native client distances. |

## PCK, world and executable evidence

The asset scan was performed against these exact staged files (SHA-256):

| Asset | SHA-256 | Relevant observations |
|---|---|---|
| `configs.pck` | `827018bfb993eaf346789378b2527ca427fedd156c24070ce02ca698df61c0bc` | 72 entries. `configs/autofamilyconfigs/autoscene.cfg` has `sky_id_day=0`, `sky_id_night=1`, fog enable and day/night fog keys. `sky.lst` names clear, cloudy and sunset entries. These are AutoFamily/editor configuration resources, not proven main-world regional settings. |
| `textures.pck` | `80f7dd27246011add8ee227bd11a5a096bddcf02e420b7a355986ace2e02d101` | 1,038 entries, 59 distinct `textures/sky/` subdirectories. Contains `textures/sky/cloud.tga` (4,194,348 uncompressed bytes), `textures/rain.tga`, `textures/snow.bmp`, `textures/sky/rain/`, `textures/sky/rain02/`, and the three folders currently hard-coded in `SkyGpu` (`clear`, `evening`, `night2`). |
| `gfx.pck` | `d121fe967b9485ce0ef088a4935d395f02f4b17e16a8346d711dd3681be80f49` | 10,104 entries; includes `gfx/textures/rain.tga`, `gfx/textures/snow.bmp`, `gfx/textures/粒子/rain.tga`, `rainlight.tga`, `snow.bmp`, and `snowflake3.dds`. This proves available effects/art, not a map-authored weather schedule. |
| `surfaces.pck` | `48f1b16683fee1d75890dd86e55873604694360122b54bbcd18a7cb654b1b199` | 3,724 entries; no filenames matched sky/weather/cloud/fog/rain/snow in the archive-name scan. |
| `world.ecwld` | `6a763c0ea0ac5717adf0a806cc2d2c09e86a4e4a809df60fc8ed0f9a5c319fc8` | v12 world grid 176×128 (22,528 blocks), 23,009 box references, 2,275 placed scene effects and 163 distinct effect paths. The survey found no ASCII-named rain/snow/cloud/fog/weather/wind effect paths; many effects have Chinese names, so this is not proof that all weather-like content is absent. `valid_time` counts: 0/day 154, 1/night 676, 2/all-day 1,445. |
| `world.ecbsd` | `16d31970c49f89c836c0cb932e60e9f4c102a87ba8ac395b770915c7343156a8` | v12, 99 referenced unique environment boxes (11 local, 88 default/global); all 99 have 5,000 ms transition duration. Flags histogram: `{0:5, 2:1, 4:1, 8:2, 16:7, 18:2, 24:79, 26:1, 28:1}`. The parsed 0xe0-byte box fields cover bounds, fog, ambient/diffuse, transition, direction and flags; no sky-set/weather identifier was found in the implemented layout. |
| `elementclient.exe` | `a290998dc13cc5558b54bda3f65d099f95bc0702e54a0613c6d7669f35c6cdb0` | 6,000,640 bytes; exact known 1.4.2 executable fingerprint. Strings include `CECWorld::LoadSky`, `Textures\\Sky\\%s`, `sky_id_day`, `sky_id_night`, `fog_start_day`, `fog_end_day`, `fog_color_water`, `rain.tga`, `snow.bmp`, and `A3DRain::Init()`. The rain initializer at 0x428ee0 constructs the rain object using `rain.tga` (0x428ffa), and a snow object using `snow.bmp` (0x429174). These establish client support, but not a world/region selection source or native weather tick period. |

The 258,594,899-byte Drive `world.zip` exceeds the connector's 100 MiB per-file transfer limit ([Drive file](https://drive.google.com/file/d/1GGoyeZUfLElugAvUNpAFsjA3P_3RcLup/view)). The corresponding loose `world.ecwld` and `world.ecbsd` files were available separately and were used for the survey and parser evidence. `textures.pck` was reconstructed from the supplied two-volume RAR in the ignored workspace; it was not added to Git. Extraction command (from the built 7-Zip `Alone2` directory): `./_o/7zz x -y -o/home/user/Perfect-World-Exlorer/google_drive /home/user/Perfect-World-Exlorer/google_drive/textures.part1.rar` — success, 117,047,648 bytes.

## Runtime behavior and regression coverage

- **Region precedence:** `Environments::endpoints` now iterates overlapping boxes into distinct local/default slots instead of globally sorting by distance. Local wins for fog/colour; the default independently supplies celestial direction/flags. If no default is present, the recovered constructor fallback is retained rather than using local flags/direction. Streamed selection continues to use `ActiveBoxMap::select_pair` and its hash/load order. New assertions in `client_v12_box_fog_and_priority` cover overlapping default + local records and local-only fallback.
- **Weather timing evidence:** `CECWorld` adds frame delta to `0x9b3ed4` and, while the accumulator is strictly `> 33 ms`, updates both optional weather objects through vtable `+0x48` and subtracts 33 (`0x429378..0x4293c7`). `client_weather::WeatherClock` reproduces that strict boundary, catch-up, and wrapping DWORD accumulator; tests cover 33/34 ms, multiple updates and wrap. Separately, `Scene::effect_visible` uses scene-effect `valid_time`: modes 0/1 switch strictly around night factor 0.5 and mode 2 is always eligible. That is placed scene-effect eligibility, not global weather selection.
- **Fog per view:** `normal_reflected_underwater_and_clipped_views_keep_distinct_regional_fog` covers main/reflection camera fog, always-enabled underwater fog, plane-relative clipped refraction fog, the below-surface air-fog override, and the fact that disabling atmospheric fog does not modify the authored fog endpoints. Existing `fog_matrix` material/pass rows keep terrain/grass/BMD exponential refraction distinct from table fog and the per-material ranges explicit.
- **Above-water visibility:** `ViewSettings::far` controls the projection far plane, while `atmospheric_fog` controls table fog separately and defaults off. The above-water no-fog presentation therefore remains intentional and explicit; no native max distance was inferred from the current app defaults.

## Commands and results

Executed from the repository root:

```text
python3 tools/region-weather-survey.py
  PASS; reports PCK counts/assets, v12 world dimensions/box and effect counts,
  default/local flags and 5,000 ms transitions, and executable strings.
python3 -m py_compile tools/region-weather-survey.py
  PASS.
python3 tools/test_phase1_ready.py
  PASS; consistent=true, errors=[], status totals 26 Implemented / 49 Partial / 1 Not implemented.
python3 tools/test_eight_phase_status.py
  PASS; 7 tests.
python3 tools/check-eight-phase-status.py
  PASS; 5 tests.
git diff --check
  PASS.
```

Rust regressions were **not executable in this sandbox**: `cargo fmt --all -- --check` returned `/bin/bash: cargo: command not found`; `rustc` and `rustfmt` are also absent. The Rust tests to run in the configured CI/toolchain are:

```text
cargo fmt --all -- --check
cargo test --locked --bin perfect-world-explorer environment::tests::client_v12_box_fog_and_priority
cargo test --locked --bin perfect-world-explorer fog_matrix::tests::normal_reflected_underwater_and_clipped_views_keep_distinct_regional_fog
cargo test --locked --bin perfect-world-explorer scene::tests::effect_validity_retains_native_strict_half_day_boundary
cargo test --locked --bin perfect-world-explorer client_weather::tests
```

No Rust test result is claimed here.

## Changed files

- `src/environment.rs` — split local/default selection in the non-streamed path; regression for overlap and fallback.
- `src/fog_matrix.rs` — per-view fog/far-clip regression.
- `src/client_weather.rs` — recovered native 33 ms weather-object clock and fixed-step tests (timing only; no visual runtime).
- `src/main.rs` — registers the timing module.
- `tools/region-weather-survey.py` — repeatable client/world evidence survey; expects ignored supplied assets under `google_drive/`.

## Remaining gaps

1. Identify the runtime main-world sky/weather settings source and how it maps sky IDs to the 99 environment boxes or another world/terrain record; then replace `SkyGpu`'s fixed experimental three-set choice with that verified selection.
2. Recover native cloud-layer, rain/snow creation/update/render order, effect lifetimes/tick cadence, weather transitions, camera-height/view admission and reflection rules before implementing P5.05. `A3DRain::Init` and texture names alone are insufficient to choose or animate any effect safely.
3. Trace all native region-selection ties and stream/load order for overlapping local boxes. The compatibility path now preserves local-over-default precedence, but its overlapping-local tie order is explicitly only stable export-ID order.
4. Finish P5.06's material/weather-specific fog variants and native distances. Keep above-water far clipping and the opt-in fog choice separate until native values/enable policy are evidenced.
5. Validate the new Rust tests under the repository's Rust/GPU CI toolchain and, when available, compare main-world sky/weather against a live 1.4.2 client.
