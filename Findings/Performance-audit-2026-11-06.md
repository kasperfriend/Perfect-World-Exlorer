# Performance audit — 2026-11-06

Two things here: the reported default-marker bug on the time-pass speed slider,
and a sweep of the whole viewer for work that can be removed without changing a
single visible pixel. Every change below is either an allocation that is now
reused, a lookup that is now indexed, or an environment read that is now done
once; no drawing rule, shader constant, LOD threshold, draw order or texture
resolution is touched.

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
| 2 | `main::terrain` block loop | seven fresh `Vec`s per block (decoded heights, narrowed values, decoded normals, narrowed normals, colour section, and the grid-sized `heights`/`normals` handed to the builder) | hoisted and `clear()`ed per block | same values, same order, same builder calls; only the allocations are reused |
| 3 | `terrain_residency::visible_runs` | a fresh `Vec` per view per frame (main, reflection, above-water, minimap) | `visible_runs_into` refills a caller-owned buffer; `visible_runs` stays as the owned wrapper | the buffer is emptied first and refilled in span order, so the runs are byte-identical (test: `visible_runs_into_reuses_the_buffer_without_keeping_the_old_answer`) |
| 4 | `terrain_gpu::visible_far_runs` | same, for the far mesh | `visible_far_runs_into`, same buffer rule | identical runs |
| 5 | `far_trees_gpu::update_resident_lods` | three fresh `Vec`s **per tree group** on every frame the camera moves | one trio for the whole pass, cleared per group | same instances, same batch ranges, same order. The buffers stay one-group sized, so the "do not keep the world-sized staging allocation" rule for `staging` is untouched |
| 6 | `gpu` UI | the FX status label built its long "Loader queue" tooltip string — two nested `format!`s and two `collect::<String>()`s — on every frame the menu was open | built only on the frame the label is hovered | the tooltip text and its `on_hover_text` path are unchanged |
| 7 | `gpu` hot paths | `std::env::var_os` on every mouse-motion device event, every opaque draw pass (twice) and every frame in the water/UI paths — each call takes the process environment lock | `env_switch` over `OnceLock`, resolved once per switch | the environment cannot change while the viewer runs; the same flags are answered |

Measured expectations, not benchmarked frame times (no GPU in this sandbox):

* (1) is the big one for loading. A budget-sized cache over the shipped corpus
  evicts on most stores, and each eviction used to hash the entire name table.
* (2) removes ~7 allocations per terrain block — thousands per world load.
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
| Terrain cache compression (`zlib` on every block write) | Kept: it is what makes the on-disk page object small, and the compression cost is the load's dominant term either way. |

## 4. Validation

Everything above is covered by the existing CI: `cargo check --all-targets`, the
requirement-status and GFX-light audits, the viewer unit tests (now including the
slider-marker and cache-alias tests), the bounded terrain RAM residency check,
the Phase 1 shader regressions, the terrain per-view GPU check, and the Windows
x64 build. No visual output is altered, so no new frame comparison is needed.
