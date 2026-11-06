# Performance audit — 2026-11-06

Three things here: the reported default-marker bug on the time-pass speed slider,
a sweep of the whole viewer for work that can be removed without changing a
single visible pixel, and the reported lens flare sitting slightly above the sun.

Every optimisation below is an allocation that is now reused, a lookup that is
now indexed, an environment read that is now done once, or a worker that now
runs beside the loader instead of in front of it; no drawing rule, shader
constant, LOD threshold, draw order or texture resolution is touched. The flare
item (§4) is a correction: the chain was anchored on the point the sun's quad is
emitted from rather than on the disc the quad paints.

## 1. The slider default marker sat at the wrong value

*Reported:* the time-pass speed slider's default tick sat at 0.1 instead of at
1.0.

`marked_slider` painted the "default" tick with a **linear** fraction on every
slider, including the egui sliders that carry `.logarithmic(true)`. egui spreads
a logarithmic track by `ln(value)`, so the `1.0` default of the
`0.1..=600` speed range landed at `(1.0 - 0.1) / (600 - 0.1) = 0.0015` — the far
left of the track, which reads as "the default is 0.1". Flight speed and mouse
sensitivity, which are also logarithmic, had the same defect.

Fixed by moving the arithmetic into `slider_mark_fraction`, which uses the same
mapping egui uses (`ln(default / min) / ln(max / min)`) when the track is
logarithmic, and by passing the logarithmic flag for the time-pass slider, which
was the only call site that declared it on the `egui::Slider` but not to the
painter. The fraction is also guarded against a degenerate range
(`max <= min`) and non-finite input, where it now falls back to the left edge
instead of producing a NaN x-coordinate. Unit tests pin the three cases: the
logarithmic position of `1.0` in `0.1..=600` (0.265), the linear position, and
the degenerate range.

Nothing else about the slider changes: same label text, same tick colour, same
geometry.

## 2. Applied optimisations

| # | where | before | after | why it is invisible |
| --- | --- | --- | --- | --- |
| 1 | `objects::TextureCache` | `evict_unpinned` removed the evicted hash's names with `by_name.retain(..)` — a full scan of every reference name in the cache (tens of thousands during a whole-world load) for **every** evicted texture, on every store past the budget | a `aliases: HashMap<u64, Vec<String>>` reverse index; eviction drops that hash's own names | the same names are removed; the resolved-name set is identical. A defensive `retain` fallback keeps the exact answer if the two tables ever disagreed |
| 2 | `main::terrain` block loop | seven fresh `Vec`s per block (decoded heights, narrowed values, decoded normals, narrowed normals, colour section, and the grid-sized `heights`/`normals` handed to the builder) | hoisted into a `BlockScratch` that is `clear()`ed per block, one per decoding worker (see (8)) | same values, same order, same builder calls; only the allocations are reused |
| 3 | `terrain_residency::visible_runs` | a fresh `Vec` per view per frame (main, reflection, above-water, minimap) | `visible_runs_into` refills a caller-owned buffer; `visible_runs` stays as the owned wrapper | the buffer is emptied first and refilled in span order, so the runs are byte-identical (test: `visible_runs_into_reuses_the_buffer_without_keeping_the_old_answer`) |
| 4 | `terrain_gpu::visible_far_runs` | same, for the far mesh | `visible_far_runs_into`, same buffer rule | identical runs |
| 5 | `far_trees_gpu::update_resident_lods` | three fresh `Vec`s **per tree group** on every frame the camera moves | one trio for the whole pass, cleared per group | same instances, same batch ranges, same order. The buffers stay one-group sized, so the "do not keep the world-sized staging allocation" rule for `staging` is untouched |
| 6 | `gpu` UI | the FX status label built its long "Loader queue" tooltip string — two nested `format!`s and two `collect::<String>()`s — on every frame the menu was open | built only on the frame the label is hovered | the tooltip text and its `on_hover_text` path are unchanged |
| 7 | `gpu` hot paths | `std::env::var_os` on every mouse-motion device event, every opaque draw pass (twice) and every frame in the water/UI paths — each call takes the process environment lock | `env_switch` over `OnceLock`, resolved once per switch | the environment cannot change while the viewer runs; the same flags are answered |
| 8 | `main::terrain` block decode | one thread decoded a whole tile: every block's zlib inflate, delta walk, normal decode and vertex-colour check ran back to back on the loader thread, which is the dominant term of a cold start (and of the resident "All to VRAM" build) | `decode_block` is extracted and driven by `decode_blocks`: one worker per core (capped, `PW_TERRAIN_THREADS` pins it) decodes blocks independently, and the caller inserts them **in block order** as they line up, so the builder sees the same sequence as before | identical bytes reach `Builder::block` in identical order; only which thread produced them changed. Per-worker scratch replaces the old shared scratch, so the allocation count stays at two per block. `PW_TERRAIN_THREADS=1` restores the old loop exactly |

Measured expectations, not benchmarked frame times (no GPU in this sandbox):

* (1) is the big one for a paged load. A budget-sized cache over the shipped
  corpus evicts on most stores, and each eviction used to hash the entire name
  table.
* (2) removes ~7 allocations per terrain block — thousands per world load.
* (8) is the big one for a cold start: the block loop is pure per-block work
  (inflate, delta walk, normal decode), so it scales with cores — 8 cores is
  roughly an 8x shorter terrain decode, and it is the phase that gates the
  resident "All to VRAM" build too.
* (3)–(5) remove per-frame allocations from the draw path; (6)–(7) remove
  per-frame and per-input string and environment work from the UI/event path.

## 3. Candidates examined and deliberately not changed

| candidate | verdict |
| --- | --- |
| `minimap` pass every presented frame | Left alone. The comment on it is explicit: the 350 ms timer it replaced capped minimap motion under 3 FPS. It is a live top-down render of animated water and the moving clock, so any "skip when idle" rule would freeze visible motion. |
| `far_trees_gpu` `staging` reuse | Left alone on purpose: the resident path deliberately re-compacts `staging` so the former world-sized buffer is released when the cutoff drops placements. Reusing it would keep that allocation alive for the whole session. |
| Per-frame `create_view` (scene depth, post chain, display gamma, present encode, surface) | Left alone. Caching the views is a few hundred nanoseconds per frame but every one of them has to be invalidated when its texture is recreated on resize, and a missed invalidation draws into a stale target — a real visual break for a tiny gain. |
| `terrain_lod::window_lod` scratch (`levels`, `cases`) | Deferred. Two small allocations of 625 and 2500 bytes per frame; the same `_into` pattern used for the visibility runs would apply, but the function also feeds the rebuild comparison, which would need its buffers threaded through the frame loop. |
| `lod_window`/`LodState` per frame | Kept. The recovered `0x77e360` rule is evaluated for every resident block on every frame by design (it is what makes the window follow the camera), and the window is at most 25 x 25 blocks. |
| Water `visible` batch list (clone + sort per frame) | Kept. The batch list is small and the sort is what gives back-to-front water blending; indices instead of clones would be a churn-only change. |
| `texture budget` / `terrain budget` `std::env::var` | Not hot: each is read once when the cache or the resident grid is created. |
| GPU instancing for animated world models, one-pass shadow/reflection merging, minimap caching | Out of scope: structural renderer changes, not "same pixels, less work". |
| Terrain cache compression (`zlib` on every block write) | Kept: it is what makes the on-disk page object small. The encode now runs on whichever worker decoded that block (see (8)), so it costs a thread rather than the whole load. |
| Parallel texture decode in `world_objects::run` (32-id batches through `load_ids`) | Not taken. The decode itself is a good candidate, but the batch loop shares one `Objects` cache, one resident builder and one package file handle per batch; splitting it means a second handle per worker and a locked cache, which is a structural change to the streaming worker, not a local one. |

## 4. The flare chain sat a disc-radius above the sun

Not a performance item, but it was found while auditing the sky pass and is in
the same commit.

The sun's recovered template is a 20-unit quad emitted at `direction * 100` whose
vertices run `y 0 .. -20`, so the **emitted point is the quad's top edge** and the
painted disc hangs `half` (= `sun_ndc_half(FOV_DEGREES)`, 0.143 NDC at 70 degrees)
below it. `celestial` already applies that shift to its own `uv`, which is why
the disc and the sunset land where the client draws them. The flare chain did
not: `flare_lanes` returns the raw projected NDC of the emitted point and
`update` wrote it straight into `glow_center.zw`, so every ghost sprite was
anchored one disc-radius too high — the "flare is a bit above the sun" report.

`flare_anchor` now drops the NDC `y` by `half * (1 + 2 * night)`, mirroring the
`1 + 2 * n` swell `celestial` applies to the same quad, and `update` writes that
into `glow_center.zw` before the uniform is uploaded. The shader is unchanged:
it already reads `glow_center.zw` as the chain's anchor, and the occlusion taps
now sample the disc's centre instead of its top edge, which is where they belong.

Test: `the_flare_chain_anchors_on_the_painted_disc` (the anchor is one half
extent below the projected point, three at full night, clamped outside 0..1, and
a camera pointed at the sun anchors exactly `half` below the screen centre); the
existing `the_flare_chain_hangs_where_the_sun_projects` still pins the projection
itself.

## 5. Validation

Everything above is covered by the existing CI: `cargo check --all-targets`, the
requirement-status and GFX-light audits, the viewer unit tests (now including the
slider-marker, cache-alias and flare-anchor tests), the bounded terrain RAM
residency check, the Phase 1 shader regressions, the terrain per-view GPU check,
and the Windows x64 build.

The flare correction is a screen-space shift of an existing overlay, so the only
frame that changes is one with the sun on screen; the geometry itself (the 15
sprites, their sizes, their UVs, the `t` ladder and the sun→centre line) is
untouched, which is why the existing size/placement tests still apply.
