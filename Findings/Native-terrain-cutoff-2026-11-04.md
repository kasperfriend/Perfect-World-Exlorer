# Optional native terrain cutoff

World → **Original-game terrain cutoff (saves memory)** restores the camera-centred, finite native block window. It is a live, persisted checkbox, off by default to preserve existing settings. The existing block-window slider controls its extent (default radius 7, up to 15×15 blocks). Enable **Client atmospheric fog** separately for the intended combination. Authored fog is not modified: some locations/radii can still expose the edge.

## Actual allocation changes

- With cutoff enabled, the all-world step-4 mesh is neither generated nor uploaded. Enabling it live drops the old far vertex/index buffers and visibility spans. Reflections, refraction, realistic maps and the water-edge pass all share this absence; none can silently restore the far terrain.
- Native near blocks retain their original resolutions, samples and internal edge stitching. The viewer-only outer stitch to the removed far mesh is disabled.
- Blend-mask payloads are loaded only for areas intersecting the resident block window plus a one-area safety halo. Their original global area IDs, layer order, texture IDs, projection, UVs and native mask resolution remain unchanged. Only the atlas slot IDs are repacked. Whole-world mask pages and both old bind groups are replaced, not retained behind a visibility flag.
- Moving into a different material-area footprint loads its masks on the existing worker. Geometry and its matching material bindings are installed together. Ordinary camera/LOD changes within the same footprint do not rebuild the atlas. Turning cutoff off reloads all masks and rebuilds the far mesh on that worker.
- Colour textures, shaders, pipelines and global day/night lightmaps are reused rather than rebuilt every time the window moves. Worker upload payloads are released after upload; queue completion retires old GPU references. During a transition, old/new mask atlases can overlap temporarily to keep the displayed scene valid.

The World panel reports **allocated blend-mask MiB and far-geometry MiB** for the installed state, not merely the requested checkbox state. These are explicit resource sizes, **not total viewer RSS** or a guarantee about an integrated GPU's driver residency. One 4096² R8 mask page is 16 MiB. The saving depends on the world's layer counts and the chosen window; an almost empty world or a window covering the entire map cannot promise a large mask saving. Global lightmaps, terrain colour textures and other scenery still cost memory. The whole-world scenery checkbox remains independent: this is a terrain cutoff, not an eviction policy for buildings or trees.

## Validation

Dependency-free tests cover material-area coverage, clamped borders, bounded native-window footprint, saved settings/backward compatibility and all four independent fog/cutoff combinations. The viewer material test checks exact retained mask values and stable global IDs while cold payloads and worker ownership are removed. Existing triangle/edge and production shader regressions remain applicable. Full locked application/build and shader validation run in PR CI; no real-client RSS measurement is claimed here.
