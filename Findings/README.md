# Findings index

This folder preserves research and open questions for the inspected client build. Addresses, counts and observations refer to that build; they should not be generalized to other versions without verification. Source data is not included. Historical entries reflect what was believed or implemented at the time and may be contradicted by later work.

- [World-loading optimization — 4 November 2026](World-loading-optimization-2026-11-04.md): compact terrain loading, shared model resources, redundant-work removal, terrain seam/tiling repairs and glow alignment; includes verification and hardware limits.
- [Full eight-phase reassessment — 2 November 2026](Eight-phase-reassessment-2026-11-02.md): current 76-point evidence matrix, implemented deliverables, partial implementations and remaining acceptance.
- [Known unsupported client paths](Unsupported-client-paths.md): explicit nonempty register and completion policy.
- [Eight-phase status](Eight-phase-status.md): the requested requirements, with explicit completion gates and no estimated parity percentage.
- [Phase 1 completion checks](Phase-1-completion-checks.md): an explicit “Is it finished?” check for every Phase 1 requirement, including remaining gates.
- [Phase 1 audit — 2 November 2026](Phase-1-audit-2026-11-02.md): initial audit plus corrected current source table; historical validation notes are retained below.
- [Phase 2 Agent A report](Phase-2-Agent-A-report.md): terrain block window, three resolution levels and the block-edge/level seam rule recovered from the 1.4.2 client, with executed geometry checks and the remaining assumptions.
- [Phase 1 Agent 3 report](Phase-1-Agent-3-report.md): postprocess pass matrix, output color/gamma boundaries and the complete per-pass fog matrix (including water surface and shoreline), with CI-executed evidence and the recorded limits.
- [Water 100% parity plan](Water-100-percent-parity.md): implementation goals, current work and unresolved gates. Water is **not** complete.
- [Research log 01 — foundations](Research-log-01-foundations.md): archive and world formats, terrain, SpeedTree, water and early renderer work.
- [Research log 02 — rendering and lighting](Research-log-02-rendering-and-lighting.md): lighting, vegetation, maps, postprocessing, model formats and memory research.
- [Research log 03 — water](Research-log-03-water.md): normal animation, reflection/refraction, shoreline, underwater and alternate material investigations.
- [Parity assessment](Parity-Assessment.md): prior system-by-system assessment; historical details may be superseded by research logs.
- [Roadmap](Roadmap.md): implementation gaps and next investigations; some items reflect earlier milestones.

These are engineering notes, not proof of visual equivalence or an independent verification of original engine behavior. Use legally obtained client files for further research and do not commit them.

- [RAM-first lossless terrain paging follow-up (2026-11-04)](RAM-paging-2026-11-04.md): block-at-a-time disk backing, bounded shared hot set, smaller texture LRU and streamed PNG exports.
- [GFX effect container: grammar, runtime player and corpus verification (2026-11-05)](Gfx-effects-reverse-engineering-2026-11-05.md): the full `.gfx` grammar with its version gates and element census, the controller/particle/ribbon runtime with native addresses, cross-verification against the static decal reader, the whole-archive sweep result, the four bugs that verification found, and what remains interpreted rather than proven.
