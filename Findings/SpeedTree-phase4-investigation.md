# SpeedTree Phase 4 investigation (partial)

This is an asset inventory and static-disassembly investigation, **not** a
runtime validation or implementation of P4.01–P4.03. The existing viewer still
uses approximate LOD transition widths and sampled leaf motion. The companion
`SpeedTree-world-asset-survey.tsv` inventories all 59 *world type records*,
but does not survey their runtime materials, LODs, billboards, or animation.
Those must not be inferred from the asset hashes.

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
* `GetDiscreteBranchLodLevel` (`0x10022260`) and
  `GetDiscreteFrondLodLevel` (`0x100222b0`) depend on continuous level and
  the runtime's respective LOD counts; `GetDiscreteLeafLodLevel`
  (`0x10022300`) has a separate drop-to-billboard branch. This is not yet a
  proof of alpha transitions, hysteresis or what the client actually draws.
  `GetTransitionValues` is at `0x100254f0`, `GetGeometry` at `0x10024680`.
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
The DLL was not executed, the split texture archive was not extracted, and no
visual or GPU comparison was made. The environment had no Rust toolchain,
Wine or Windows cross compiler, so Rust builds and native probes were not run.
A native 32-bit client/DLL probe or a verified emulator should capture every
type's material arrays, actual geometry, alpha, LOD transition behavior and
wind at multiple times and strengths before production changes and material,
LOD-boundary and animation regressions can be accepted.
