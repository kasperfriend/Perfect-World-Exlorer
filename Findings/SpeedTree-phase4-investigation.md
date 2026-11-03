# SpeedTree Phase 4 investigation (partial)

This is an asset inventory and emulation/static-disassembly investigation,
**not** an implementation of P4.01–P4.03. The existing viewer still uses
approximate LOD transition widths and sampled leaf motion. The companion
`SpeedTree-world-asset-survey.tsv` inventories all 59 *world type records*.
`SpeedTree-emulated-materials.jsonl` records DLL material getter results for
all 59 after emulated SPT loading; these are experimental and are not a survey
of client-applied material state, LODs, billboards, or animation.

## Reproduction and evidence

Using the client files from Drive, whose hashes identify the binaries studied:

| file | SHA-256 |
|---|---|
| elementclient.exe | a290998dc13cc5558b54bda3f65d099f95bc0702e54a0613c6d7669f35c6cdb0 |
| speedtreert.dll | 7f0a401688f63c520ab717a4b9939705e5aac635a649a8cfb5df6af19681b46f |
| trees.pck | 6cbe043e75f9c249c3720f986e7424a3cf8cd4727c5d0714549e2bdb901247f8 |
| shaders.pck | 34a556cc2f69f54f52dc244d4d4249350cb99b81218a98c7dc3362acf092061b |

The `world.ecwld` MD5 is `eb5e4c7be58548341dc3cabf1359949e`. PCK
records in the TSV are hashed after decompression. Run:

```sh
python3 tools/speedtree/asset_survey.py google_drive/world.ecwld google_drive/trees.pck > Findings/SpeedTree-world-asset-survey.tsv
python3 -m unittest discover -s tools/speedtree -p 'test_asset_survey.py' -v
objdump -d -Mintel --start-address=0x656270 --stop-address=0x6562e0 google_drive/elementclient.exe
objdump -d -Mintel --start-address=0x100282b0 --stop-address=0x10028380 google_drive/speedtreert.dll
```

The inventory reports 59 rows, 57 distinct SPT content hashes and 37 distinct
composite content hashes; it verifies each referenced PCK record exists and
its decompressed length matches the directory. Paths preserve the ECWLD type
order, which is the cache type ID. This is not an enumeration of instances.

## Experimental material getter survey

A follow-up executes DLL initialization, constructor, `LoadTree`, and the
branch, frond and leaf material getters for each world SPT under Speakeasy /
Unicorn. The resulting `SpeedTree-emulated-materials.jsonl` has 59 records;
all processes returned success and duplicate SPT hashes yielded identical
arrays. The 13 raw floats contain 5, 11 and 13 distinct branch, frond and
leaf arrays respectively. Some leaves have non-zero values in slots 6–8,
9–11, or 12, so assuming one uniform material is not justified.

This is **not verified Windows output**: the emulator substitutes CRT
allocation, string scanning, `_ftol`, `atof`, trigonometric and pow routines,
uses fake file-descriptor responses, and explicitly runs the DLL's initializer
array because Speakeasy's `_initterm` is a no-op. Only SPT loading and getters
are measured, not Compute or the renderer, and array slots have not been
independently mapped to client-applied material state. Do not use these raw
values as a production fix without validating ABI, client call paths and the
substitutes against an independent native run. For example, an experimental
Compute attempt produced a questionable type-0 height and has deliberately
not been used as evidence.

To reproduce (requires `pefile`, `speakeasy-emulator` and `unicorn`):

```sh
mkdir -p google_drive/research/materials
seq 0 58 | xargs -P 4 -I '{}' sh -c 'PYTHONPATH=.cache/py python3 tools/speedtree/probe_materials_emulated.py google_drive/world.ecwld google_drive/trees.pck google_drive/speedtreert.dll --type "$1" > "google_drive/research/materials/$1.json" 2> "google_drive/research/materials/$1.err"' _ '{}'
```

Result: exit 0, 59 JSON files, no stderr; one independent rerun of type 0
matched its recorded output. `PYTHONPATH=.cache/py python3 -m unittest discover
-s tools/speedtree -p 'test*.py' -v` passed seven tests, including ordinary
finite-input checks of the substituted x87 arithmetic. Those tests do not
validate MSVCRT edge cases or geometry. World SHA-256 is
`6a763c0ea0ac5717adf0a806cc2d2c09e86a4e4a809df60fc8ed0f9a5c319fc8`.

## Static observations

* Client `0x656270–0x6562c8` retrieves the computed bounding box, takes
  `max(max_z - min_z, 5.0)`, multiplies it by the owner field `+0x228`, then
  calls `SetLodLimits` with near = that product × 5 and far = that product ×
  16. The operands at `0x8d5ee8` and `0x8d7320` decode as 5 and 16.
  A second call site is `0x657d92`. The meaning and update path of owner
  `+0x228` still need end-to-end validation; it must not be equated blindly
  with viewer quality settings.
* `CSpeedTreeRT::ComputeLodLevel` export `0x10022050` delegates to
  `0x100282b0`, which takes all three components of the camera-relative
  vector and computes an approximate square root using the float-bit operation
  `(bits(distance_squared) >> 1) + 0x1fc00000`, before applying near/far
  limits and clamping. Thus the viewer's XZ-only distance and ordinary
  Euclidean sqrt would not be exact. NaN/zero and instruction-level x87
  behavior remain unvalidated against a running DLL.
* Follow-up: `PYTHONPATH=.cache/py python3 tools/speedtree/probe_lod_unicorn.py google_drive/speedtreert.dll` executed that isolated DLL routine under Unicorn (not Wine or Windows) with near=50, far=160. It returned 1 at distance 25, 0.986079574 at 50, 0.517613649 at 100, and 0 at 160. A vertical-only displacement and a translated-camera case produced the same result as the corresponding horizontal relative displacement. This confirms the approximate-sqrt behavior in this emulator; the boundary at exactly the nominal near limit is already below 1. The script checks the exact DLL SHA-256 and uses `pefile` and `unicorn` Python packages. These fixtures are not independent native comparison and do not determine component transitions.
* `GetDiscreteBranchLodLevel` (`0x10022260`) and
  `GetDiscreteFrondLodLevel` (`0x100222b0`) depend on continuous level and
  the runtime's respective LOD counts; `GetDiscreteLeafLodLevel`
  (`0x10022300`) has a separate drop-to-billboard branch. This is not yet a
  proof of alpha transitions, hysteresis or what the client actually draws.
  `GetTransitionValues` is at `0x100254f0`, `GetGeometry` at `0x10024680`.
  The isolated getters were subsequently emulated using synthetic structures:
  `PYTHONPATH=.cache/py python3 tools/speedtree/probe_discrete_lod_unicorn.py google_drive/speedtreert.dll`.
  For three levels with drop-to-billboard disabled, branch, frond and leaf
  produce indices 0 at level 0.7, 1 at 0.6, and 2 at 0.3. With leaf
  drop-to-billboard enabled **and** a positive billboard count, the leaf
  getter uses an extra slot: level 0.25 and 0 return index 3; clearing either
  prerequisite restores index 2 at 0.25. The probe substitutes a small x87
  `_ftol` CRT routine and has not been independently checked on Windows.
  An index 3 here is not proof the client renders a billboard: the client
  still needs to be traced through its visibility and draw paths. At client
  `0x656084`, EDI is zeroed and is not reassigned before the call to
  `SetDropToBillboard` at `0x6562db`, so this initialization path passes
  false (subject to normal x86 callee-saved register conventions). The two
  branch, leaf and frond lighting setters at `0x6561b4`, `0x6561c0`,
  `0x6561cc` receive the same zero. The full draw-path and any
  later state changes remain to be traced.
* At `0x6561d9`, `0x6561e6`, `0x6561f3` the client sets all three wind
  methods to enum 2, and at `0x656200` sets one leaf rocking group. This
  alone does not prove that branch/frond vertices move at runtime. The
  manager calls `SetTime` at `0x650080`; instance update calls
  `ComputeLodLevel` at `0x6569fc` and `SetWindStrength` around `0x656a1e`.
* The `shaders.pck` records `shaders/vs/treebranchvs.txt` and
  `shaders/ps/treebranch.txt` describe a tangent/binormal normal-mapped
  branch path with directional and ambient terms, `mul_x2`, and a fourth
  power specular term with constant `(0.5, 0.5, 0.5, 1)`. Their presence
  does not establish when the client activates that shader. The client
  imports lighting-method setters, not the DLL material getters; material
  state and the other rendering lanes require further tracing.

## Remaining work / limitations

No production tree path was changed: incomplete static evidence is not a
safe basis for replacing the current approximations or claiming exactness.
The PWTREE07 cache has only three geometry snapshots and sampled motion; exact
runtime LOD and wind require additional data and/or a runtime implementation.
DLL SPT loading and isolated LOD routines were emulated with CRT substitutes,
but the client renderer and reliable computed geometry were not validated. The
split texture archive was not extracted, and no visual or GPU comparison was
made. The environment had no Rust toolchain,
Wine or Windows cross compiler, so Rust builds and native probes were not run.
A native 32-bit client/DLL probe or a verified emulator should capture every
type's material arrays, actual geometry, alpha, LOD transition behavior and
wind at multiple times and strengths before production changes and material,
LOD-boundary and animation regressions can be accepted.
