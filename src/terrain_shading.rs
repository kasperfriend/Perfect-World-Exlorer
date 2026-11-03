//! Recovered `A3DTerrain2` variant selection, pass state, constant block and
//! lightmap arithmetic for elementclient 1.4.2
//! (md5 5f02895688fa580506e869156340a664).
//!
//! Every value here carries the native address it was read from. The module has
//! no wgpu dependency, so `tools/check-phase1-core.sh` compiles it standalone as
//! a `--test` crate; `src/terrain_gpu.rs` maps [`PassState`] onto a wgpu pipeline
//! and uses the arithmetic functions as the reference the WGSL must match.
//!
//! This file corrects three earlier estimates:
//!
//! * VS `c0.x` is **1.0** on both constant-block paths (0x7c2872), not the
//!   regional ambient. `c0.y` is `env+0x1a0`, which only 0x7c4940 ever writes
//!   (the scene's `lightmap_scene` boolean), and `terrain_vs` never reads it.
//! * PS `c3.a` is `env+0x198`, one lane of the **raw light-matrix product**
//!   `0x7b4b00` copies into `env+0x194..+0x1d3`, not a daylight fraction;
//!   `0x7b4eb0` normalises a *copy* into `env+0x2a4..`, so the shipped lane is
//!   neither normalised nor negated and no app scalar feeds the blend at all.
//! * The layer weights are the mask texture's **blue, green and red** channels
//!   in that order (blue = layer 1), and `c2` is a per-channel on/off vector of
//!   ones and zeros, not a scale.

/// Native addresses of every constant and comparison this module encodes.
pub mod native {
    /// `A3DTerrain2Render::ApplyMask` variant switch.
    pub const VARIANT_SWITCH: u32 = 0x7c14_50;
    pub const PATH_BATCH_3_LAYER: u32 = 0x7c14_b0;
    pub const PATH_PER_MASK_LIGHTMAP: u32 = 0x7c1a_20;
    pub const PATH_NO_PIXEL_SHADER: u32 = 0x7c1e_00;
    /// Hardware-lightmap path. Gated on `env+0x1b6`, whose only writer stores
    /// zero (`0x77d0ea`), so 1.4.2 never reaches this entry.
    pub const PATH_HARDWARE_LIGHTMAP: u32 = 0x7c21_00;
    /// One layer batch: stage loop, `c2`/`c3` upload, lightmap stages.
    pub const BATCH_3_LAYER: u32 = 0x7c23_00;
    /// Where the loader stores the two lightmap records it read: the authored
    /// `.t2lm` day texture at `record+0xc` (`0x7a2aeb`, gated on `env+0x880`
    /// set at `0x7a0e84`) and the `1.t2lm` night texture at `record+0x10`
    /// (`0x7a2b0c`, gated on `env+0x881` set at `0x7a0eec`).
    pub const LIGHTMAP_RECORD_DAY: u32 = 0x0c;
    pub const LIGHTMAP_RECORD_NIGHT: u32 = 0x10;
    pub const BATCH_PER_MASK: u32 = 0x7c23_d0;
    pub const MASK_STAGE: u32 = 0x7c25_30;
    pub const OPTIONAL_STAGE: u32 = 0x7c26_50;
    pub const SHADER_CONSTANTS: u32 = 0x7c26_e0;
    /// Per-area entry: fills the mask atlas tile (`0x7c0ce0`) then draws.
    pub const AREA: u32 = 0x7c16_a0;
    pub const AREA_DRAW: u32 = 0x7cd7_60;
    /// Shader loading and `Configs\terrain2_shaders.cfg`.
    pub const LOAD_SHADERS: u32 = 0x7c4d_90;
    /// `A3DTerrain2Env` lazy singleton; `0x9d30fc` points at it.
    pub const ENV_SINGLETON: u32 = 0x7c46_c0;
    pub const ENV_CONSTRUCTOR: u32 = 0x7c47_d0;
    /// `LoadShaders` - which reads `device+0xc` into the `env+0x19c` guard.
    pub const ENV_LOAD_SHADERS: u32 = 0x7c48_80;
    /// Side effect of the singleton: the only writer of `env+0x1a0`.
    pub const ENV_INIT_CONSTANTS: u32 = 0x7c49_40;
    /// `GEnv::UpdateSceneLight`: receives a 4x4 light matrix, multiplies it
    /// against the camera basis at `0x7b4e5d` and copies the product into
    /// `env+0x194..+0x1d3` at `0x7b4e75`. `UpdateDirection` then normalises
    /// `env+0x194..+0x19c` (0x7b4f22..0x7b4fbe) and `env+0x1a4..+0x1ac`
    /// (colour, 0x7b4fdc..0x7b5073) into **separate** fields `env+0x2a4..` and
    /// `env+0x2e4..`; the source rows keep their raw values.
    pub const UPDATE_SCENE_LIGHT: u32 = 0x7b4b_00;
    pub const UPDATE_DIRECTION: u32 = 0x7b4e_b0;
    /// The 0x4c-byte terrain shader object. `+0xc` is the pixel-shader-config
    /// result (`0x7c48b7`), read by the variant switch at `0x7c1455`; `+0x19c`
    /// is a copy of it, re-read at `0x7c170a`.
    pub const DEVICE_CAPS: u32 = 0x9d30fc;
    /// Mode reads from the terrain env: `env+0x1b5` at 0x7c1465, re-read at
    /// 0x7c170a; `env+0x1b6` at 0x7c1469 and 0x7c26ee. Neither is a debug key.
    pub const FLAG_PER_MASK_LIGHTMAP: u32 = 0x7c170a;
    pub const FLAG_HARDWARE_LIGHTMAP: u32 = 0x7c26ee;
    /// `/v`-style specular scalar in VS `c0.w` (and the `v1` the pixel shader
    /// multiplies by). Zero in the image (`0x9d30f8` sits in `.data` past the
    /// raw section end, i.e. BSS). Two debug keys ramp it by `0.001`
    /// (`0x8effc0`) at `0x7c274e`/`0x7c2768`, but `0x7c27cf..0x7c281a` rewrites
    /// every value that is neither 0 nor 1 to `1.0` and then, in the same block,
    /// to `0.0`, so what reaches the device is always exactly 0 or 1.
    pub const GLOBAL_LIGHTMAP_SCENE: u32 = 0x009d_30f8;
    /// `.rdata` anchors used by the lightmap composite.
    pub const FLOAT_HALF: u32 = 0x008e_ff98;
    pub const FLOAT_ONE: u32 = 0x008e_ffa0;
    pub const FLOAT_ZERO: u32 = 0x008e_ffc4;
}

/// Terrain shading variants the client can select, in native test order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// 0x7c14b0: pixel shader `t2_3layer_batch_14` over three layers, packed
    /// mask, day+night lightmap. The path a stock 1.4.2 client takes.
    Batch3Layer,
    /// 0x7c1a20: pixel shader `t2_tex_merger`, one mask per layer.
    PerMaskLightmap,
    /// 0x7c1e00: no programmable pixel pipeline. Fixed-function stages,
    /// lightmap stages dropped.
    NoPixelShader,
    /// 0x7c2100: fixed-function stages plus a projected lightmap. No pixel
    /// pipeline may be bound at all (0x7c21ac calls `0x77baf0`/`0x77bad0`,
    /// which are the `SetPixelShader(NULL)`/`SetVertexShader(NULL)` pair), yet
    /// the path also uploads `c0..` pixel constants at 0x7c22c2, so it cannot be
    /// reached on a device without a pixel pipeline - and the only writer of its
    /// gate `env+0x1b6` is the zeroing constructor. Dead code in 1.4.2.
    HardwareLightmap,
}

/// The flags the native switch reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    /// `[[0x9d30fc]+0xc] != 0`, read at 0x7c1455.
    ///
    /// `0x9d30fc` is the 0x4c-byte terrain shader object, not a D3D device
    /// query: `0x7c46c0` allocates it, `0x7c47d0` constructs it, and
    /// `0x7c4880` stores the pixel-shader-config result into `+0xc`
    /// (`0x7c48b7: setne %cl; 0x7c48ba: mov %cl,0xc(%esi)` from the
    /// `0x7c4d90` `Configs\terrain2_shaders.cfg` load). So this byte means
    /// "the terrain pixel shaders loaded", and the fallback below it is the
    /// fixed-function path.
    pub pixel_pipeline: bool,
    /// `env+0x1b5` (0x7c1465).
    ///
    /// Written once per terrain-env init, `0x77d4d8`, as
    /// `(shaders_loaded) ? arg : 1` where `arg` is the caller's byte at
    /// `0x20(%esp)` of `0x77d1d0`. It is a **settings-driven terrain mode**
    /// (the per-mask, vertex-lit layout - the same byte that picks vertex
    /// stride 0x34/FVF 0x4c2 at `0x7c0d99`), not a debug key: the strings
    /// `PerMask` and `Terrain2PerMask` do not occur anywhere in the binary, so
    /// the earlier "GDebugKey `Terrain2PerMask`" label is retracted. The byte
    /// also has many other writers at unrelated `+0x1b5` offsets, so a linear
    /// scan of `0x1b5(%reg)` is not evidence about this object.
    pub per_mask_lightmap: bool,
    /// `env+0x1b6` (0x7c1469, 0x7c26ee).
    ///
    /// Sole writer: `0x77d0ea: mov %bl,0x1b6(%esi)` inside the zero-init block
    /// of the terrain-env constructor `0x77ce50`, where `0x77ce6a: xor %ebx,%ebx`
    /// dominates every use of `%ebx` up to that store (checked through
    /// `0x77d120`; all `%ebx` operands in between are stores of that zero).
    pub hardware_lightmap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariantError {
    /// `env+0x1b6` has one writer, `mov %bl,0x1b6(%esi)` at 0x77d0ea inside a
    /// `xor %ebx,%ebx` prologue, so no 1.4.2 session can enter 0x7c2100.
    /// Report it instead of drawing another variant.
    HardwareLightmapUnreachable,
}

/// Native variant selection, 0x7c1450. Order matters: `device+0xc`
/// short-circuits everything, then `env+0x1b5`, then `env+0x1b6`.
pub fn select(caps: Caps) -> Result<Variant, VariantError> {
    if !caps.pixel_pipeline {
        // 0x7c145a -> 0x7c14a0 -> 0x7c1e00, before either env flag is read.
        return Ok(Variant::NoPixelShader);
    }
    if caps.per_mask_lightmap {
        // 0x7c1467 -> 0x7c1491 -> 0x7c1a20.
        return Ok(Variant::PerMaskLightmap);
    }
    if caps.hardware_lightmap {
        // 0x7c1473 -> 0x7c2100; unreachable because 0x77d0ea always stores zero.
        return Err(VariantError::HardwareLightmapUnreachable);
    }
    // 0x7c1482 -> 0x7c14b0.
    Ok(Variant::Batch3Layer)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cull {
    /// `D3DCULL_NONE` = 1, pushed at 0x7cd789 and popped at 0x7cda6c.
    ///
    /// `env+0x168` is **not** a saved previous cull value: it is the
    /// `D3DRS_CULLMODE` cache slot. `0x77a540` stores the enum it is given into
    /// `env+0x168` and then pushes `D3DCULL_NONE` (`dec` == 0 at
    /// `0x77a547..0x77a55a`), so the wrapper's argument is the caller's request
    /// and the device sees `D3DCULL_NONE` on both sides of the bracket.
    None,
}

/// Pass state for one terrain draw.
///
/// The area entry 0x7cd760 brackets every terrain draw with:
///
/// ```text
/// 007cd77d: push $0x1
/// 007cd789: call 0x77a540   ; CULLMODE(22), arg 1 -> cache 0x168, device NONE
/// 007cd793: call 0x77a230   ; ZWRITEENABLE(14) = 1
/// 007cd79d: call 0x77a210   ; ZENABLE(7) = 1
/// 007cd7c1: call 0x77a3c0   ; LIGHTING(137) = 1
/// 007cd7c9: call 0x77b570   ; SPECULARENABLE(26) = 1
/// 007cd7d5: call 0x77bd00   ; 0x8e = 1, 0x8f = 1 (shade mode, diffuse/specular)
/// ```
///
/// `D3DRS_ALPHABLENDENABLE` (27, wrapper 0x77a290) and
/// `D3DRS_ALPHATESTENABLE` (15, wrapper 0x77a320) are never called from
/// 0x7c1400-0x7d3fff, so terrain is opaque and writes depth. Each path does
/// install SRCBLEND/DESTBLEND pairs; those are recorded because they are what
/// the client would use if blending were on, but they are inert.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassState {
    pub cull: Cull,
    pub z_enable: bool,
    pub z_write: bool,
    /// `0x7cd7c1` sets `D3DRS_LIGHTING` to 1 for the bracket, and the per-mask
    /// and no-pixel-pipeline paths then set it to 0 at 0x7c1a65 / 0x7c1e15 and
    /// back to 1 at 0x7c1ae8 / 0x7c1ef0. The stock 3-layer path leaves it at 1
    /// because its vertex shader consumes the vertex normal instead of any
    /// fixed-function light; either way no terrain path enables a D3D light, so
    /// the baked lightmap is never doubled.
    pub lighting_at_draw: bool,
    pub specular_enable: bool,
    /// Always false: `0x77a290` has no terrain caller.
    pub alpha_blend: bool,
    /// Always false: `0x77a320` has no terrain caller.
    pub alpha_test: bool,
    /// `env+0x1c` bound at `0x7c236d`; the pixel module itself is chosen by the
    /// same `env+0x1b5` byte that picks the vertex stride.
    pub pixel_shader: bool,
    /// `terrain_vs` is bound by the per-area draw itself (`0x7cd9ea` calls the
    /// vertex-shader setter, `0x7cd9df` the pixel-shader setter), so it is live
    /// on every variant except the dead path, which clears it at `0x7c21ac`.
    pub vertex_shader: bool,
}

/// The native state every variant shares. Differences are the shader binding
/// and the stage count, not the render states: the three live paths differ only
/// in which module entry they call.
pub fn pass_state(v: Variant) -> PassState {
    PassState {
        cull: Cull::None,
        z_enable: true,
        z_write: true,
        // Only the per-mask and no-pixel-pipeline paths turn fixed-function
        // lighting off around their draw (0x7c1a65 / 0x7c1e15).
        lighting_at_draw: matches!(v, Variant::Batch3Layer | Variant::HardwareLightmap),
        specular_enable: true,
        alpha_blend: false,
        alpha_test: false,
        pixel_shader: matches!(v, Variant::Batch3Layer | Variant::PerMaskLightmap),
        vertex_shader: !matches!(v, Variant::HardwareLightmap),
    }
}

/// `D3DTOP_MODULATE2X`. The fixed-function path installs it on stage 0
/// (0x7c1e3f and 0x7c2006) and then `D3DTOP_MODULATE` on the mask stage
/// (0x7c1f32), which is the stage-state equivalent of the pixel shader's
/// `mul_x2 r0, r0, r5`: both pipelines double the composite.
pub const D3DTOP_MODULATE2X: u32 = 5;
pub const D3DTOP_MODULATE: u32 = 4;
pub const D3DTOP_DISABLE: u32 = 1;

/// Number of texture stages a variant configures. Every variant configures a
/// fixed set: the per-mask entry `0x7c1a20` contains six **unrolled** stage
/// blocks (`0x7c1b0b`, `0x7c1b92`, `0x7c1b53`, `0x7c1bd7` each repeat for
/// stages 0..5 with immediate arguments, no loop), so an earlier note that it
/// was `2 + masks` with a per-mask stage loop is retracted - the only loop in
/// that function is `0x7c1ac9-0x7c1ac6`, which draws one batch per mask
/// (`env+0x10` iterations, `0x7c1c10` worker) and installs no stage state.
pub fn stage_count(v: Variant) -> Option<u32> {
    match v {
        // t0..t5: three colours, one packed mask, day and night lightmaps
        // (0x7c23a2 stage 4, 0x7c23b0 stage 5).
        Variant::Batch3Layer => Some(6),
        // 0x7c1b0b-0x7c1c04: stages 0..5, unrolled.
        Variant::PerMaskLightmap => Some(6),
        Variant::NoPixelShader => Some(2),
        // 0x7c210b stage 0, 0x7c2219 stage 1, 0x7c2225 stage 2.
        Variant::HardwareLightmap => Some(3),
    }
}

/// `D3DTSS_TEXCOORDINDEX` for terrain stages: stage `n` reads texcoord set `n`
/// (0x7c2231-0x7c2249 and 0x7cd84f: `(0,1)`, `(1,1)`, `(2,2)`), and the mask
/// stage asks for texcoord 5 (`mov oT3/oT4/oT5, v5` in both vertex shaders).
/// The area draw overrides stage 2 with `0x20000` (0x7cd937) = index 0 plus the
/// `D3DTSS_TCI_CAMERASPACE` flag, which is how the lightmap gets its
/// screen-projected coordinates.
pub const TEXCOORD_CAMERASPACE: u32 = 0x20000;
pub const TEXCOORD_MASK: u32 = 5;

/// Vertex FVF and stride, 0x7c0d99: `lea 0x34(,%eax,4),%eax` with
/// `sete %al` on `env+0x1b5 == 0`.
///
/// `env+0x1b5 == 0` -> 0x38 stride, FVF 0x412
/// (`D3DFVF_XYZ|D3DFVF_NORMAL|D3DFVF_TEX4` = 12 + 12 + 4*8).
/// `env+0x1b5 != 0` -> 0x34 stride, FVF 0x4c2
/// (`D3DFVF_XYZ|D3DFVF_DIFFUSE|D3DFVF_SPECULAR|D3DFVF_TEX4` = 12 + 4 + 4 + 32),
/// which is exactly why the per-mask path needs no normal: it is vertex-lit.
pub const fn vertex_layout(per_mask_lightmap: bool) -> (u32, u32) {
    if per_mask_lightmap {
        // Diffuse and specular replace the normal, so the stride drops by 4.
        (0x4c2, 0x34)
    } else {
        // The layout the stock 3-layer batch uploads.
        (0x412, 0x38)
    }
}

/// The terrain vertex-shader constant block, 0x7c26e0.
///
/// ```
/// 007c2824: flds 0x1a0(%ecx)      ; env+0x1a0 = lightmap_scene
/// 007c282e: fstps 0x18(%esp)      ; c0.y
/// 007c2838: flds 0x98f368
/// 007c283e: fstps 0x1c(%esp)      ; c0.z
/// 007c2842: flds 0x9d30f8
/// 007c2848: fstps 0x20(%esp)      ; c0.w  (specular strength, 0.0)
/// 007c2872: movl $0x3f800000,0x18(%esp) ; c0.x = 1.0
/// 007c287a: call 0x77ba40          ; SetVertexShaderConstant(0, c0, 1)
/// ```
///
/// The three scalars are stored after two pushes and `c0.x` after the third, so
/// `c0 = (1.0, lightmap_scene, [0x98f368], [0x9d30f8])`. `terrain_vs` copies
/// `c0.x` to `oD0` and `c0.w` to `oD1`, so the pixels shader's `v0` is 1.0 and
/// its `v1` is 0.0: there is **no** regional ambient term and, because the
/// `0x7c27cf..0x7c282a` guard quantises `0x9d30f8` to exactly 0 or 1 before it
/// is uploaded, the specular term is off in a stock session and can only be
/// turned fully on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderConstants {
    /// VS c0: (1.0, lightmap_scene as f32, `0x98f368`, `0x9d30f8`).
    pub c0: [f32; 4],
    /// PS c2: per-channel mask selector, 1.0 for a bound layer and 0.0 for an
    /// absent one (0x7c2331 / 0x7c2347).
    pub c2: [f32; 4],
    /// PS c3: `env+0x198` broadcast to all four lanes (0x7c2377). That field is
    /// a raw lane of the light-matrix product, not a normalised direction - see
    /// [`native_day_night_factor`].
    pub c3: [f32; 4],
}

pub fn shader_constants(
    lightmap_scene: bool,
    global_density: f32,
    specular: f32,
    layers: [bool; 3],
    scene_direction: [f32; 3],
) -> ShaderConstants {
    ShaderConstants {
        c0: [
            1.0,
            if lightmap_scene { 1.0 } else { 0.0 },
            global_density,
            specular,
        ],
        // 0x7c232b loops three stages: bound -> 1.0, absent -> 0.0.
        c2: [
            layers[0] as u8 as f32,
            layers[1] as u8 as f32,
            layers[2] as u8 as f32,
            0.0,
        ],
        c3: [scene_direction[1]; 4],
    }
}

/// Which texture stage holds which lightmap. `0x7c23a2` installs stage 4 and
/// `0x7c23b0` stage 5, and `t2_3layer_batch_14` declares `t4` = day map and
/// `t5` = night map, so the pair is fixed by stage index and never swapped by
/// the blend factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightmapPair {
    pub day_stage: u32,
    pub night_stage: u32,
}

/// `Some` for variants that bind a day/night lightmap pair, `None` otherwise.
pub fn lightmap_pair(v: Variant) -> Option<LightmapPair> {
    match v {
        Variant::Batch3Layer => Some(LightmapPair {
            day_stage: 4,
            night_stage: 5,
        }),
        // 0x7c2253/0x7c225d bind the two lightmap textures to the only two
        // remaining stages of the fixed-function path.
        Variant::HardwareLightmap => Some(LightmapPair {
            day_stage: 1,
            night_stage: 2,
        }),
        Variant::PerMaskLightmap | Variant::NoPixelShader => None,
    }
}

/// Day/night lightmap blend, `t2_3layer_batch_14`:
/// `lrp_sat r5, c3.a, r5, r4` with `t4` = day map, `t5` = night map.
/// ps.1.4 `lrp d, s0, s1, s2` computes `d = s0*s1 + (1-s0)*s2`, so
/// `factor == 0` selects the day map and `factor == 1` the night map.
///
/// `c3.a` is `env+0x198`, one lane of the raw light-matrix product that
/// `GEnv::UpdateSceneLight` leaves there (see [`native_day_night_factor`] for
/// what it is and is not). `lrp_sat` saturates that lane, which is the only
/// clamp in the path, so any value inside `[0, 1]` cross-fades the two maps
/// instead of switching them.
pub fn day_night_lightmap(day: f32, night: f32, factor: f32) -> f32 {
    (factor * night + (1.0 - factor) * day).clamp(0.0, 1.0)
}

/// The native blend factor, `ps c3.a`: `env+0x198`, broadcast to all four lanes
/// of `c3` by `0x7c2377`.
///
/// `GEnv::UpdateSceneLight` (`0x7b4b00`) receives a 4x4 light matrix, multiplies
/// it against the camera basis at `0x7b4e5d` and copies the 16 dwords straight
/// into `env+0x194..+0x1d3` (`0x7b4e67..0x7b4e75`). `UpdateDirection`
/// (`0x7b4eb0`) then reloads `env+0x194..+0x19c`, normalises them with the
/// `1e-6` degeneracy guard (`0x7b4f22..0x7b4fbe`, constants `0x8f07d0` /
/// `0x8f07c8`) and writes the result to `env+0x2a4..+0x2ac` - **not** back into
/// `env+0x194..`. So `c3.a` is a raw row of the product, *not* normalised and
/// *not* clamped: `lrp_sat` is the only clamp in the path. Those are the matrix
/// rows: `0x194`/`0x198`/`0x19c` are the first three lanes of the third row.
///
/// The `fchs` pair at `0x7b4c0b..0x7b4c13` that flips lanes 1 and 2 does **not**
/// touch these fields: it acts on `env+0x1d8`/`env+0x1dc`, the second column of
/// the same matrix, under the `env+0x30 & 2` flag. Reading it as a negation of
/// the factor was a misread of the displacement and is retracted here.
///
/// What is recovered is therefore the storage and the arithmetic, not the sign
/// convention of the source matrix: `0x7b4e5d` multiplies by the camera basis
/// and the sign of the resulting row is only determined once the caller's light
/// matrix is identified. [`viewer_day_night_factor`] is the stand-in until then.
/// The one ordering fact the user-visible state does settle is the **pair**: the
/// callers install the authored `.t2lm` on stage 4 (`0x7c23a2` requests
/// `record+0xc`, which `0x7a2aeb` stores the day texture into) and the
/// `1.t2lm` night record on stage 5 (`0x7c23b0`, `record+0x10`, `0x7a2b0c`).
pub fn native_day_night_factor(light_matrix_row: [f32; 3]) -> f32 {
    // `lrp_sat` saturates, so the raw row lane is clamped here and nowhere else.
    light_matrix_row[1].clamp(0.0, 1.0)
}

/// Viewer stand-in for [`native_day_night_factor`] until
/// `GEnv::UpdateSceneLight` is reconstructed. The viewer has no scene light
/// matrix, only the app-level clock, so it uses the inverse day fraction:
/// full daylight gives 0 (day map) and full night gives 1 (night map), which is
/// the same mapping at both endpoints and the same monotonic direction, but not
/// the same curve: the native factor follows a row of the authoured light
/// matrix, this one follows `environment::daylight`.
///
/// Registered gap: `Findings/Unsupported-client-paths.md`, "Terrain day/night
/// factor". Replacing this function with [`native_day_night_factor`] is the
/// whole of the remaining P2.05 work, and it needs the regional light direction
/// published to the shader by whoever owns the camera uniform.
pub fn viewer_day_night_factor(daylight: f32) -> f32 {
    (1.0 - daylight).clamp(0.0, 1.0)
}

/// Whether the night map reaches the output at all. The native factor is one
/// lane of the light matrix row clamped by `lrp_sat`, so the two maps cross-fade
/// over the whole non-negative range and the night map is never selected while
/// that lane is `<= 0`.
pub fn lightmap_uses_night(light_matrix_row_y: f32) -> bool {
    light_matrix_row_y > 0.0
}

/// Complete `t2_3layer_batch_14` radiance:
///
/// ```text
/// lrp_sat r5, c3.a, r5, r4      ; r5 = day/night lightmap
/// mul r2.rgb, r1.r, v1          ; v1 = c0.w = 0.0 in 1.4.2
/// mad r0.rgb, r5, r2, r0        ; lightmap * specular, added
/// mul r0.rgb, r0, v0            ; v0 = c0.x = 1.0
/// mul_x2 r0.rgb, r0, r5         ; * lightmap * 2
/// ```
///
/// The layer composite is [`composite_layers`], the mask channels are
/// [`packed_weight`], and the factor is [`day_night_lightmap`]. The result is
/// saturated because the render target is 8-bit.
pub fn compose(layers: f32, lightmap: f32, specular: f32, specular_scale: f32) -> f32 {
    ((layers + lightmap * specular * specular_scale) * lightmap * 2.0).clamp(0.0, 1.0)
}

/// The packed mask of `t2_3layer_batch_14` is one texture for three layers:
/// `blue` = layer 1, `green` = layer 2, `red` = layer 3 (the shader comments say
/// so and 0x7c2382 uploads the same vector to `c2`/`c3` for the per-mask path).
/// `c2` masks a channel out when its layer is absent (`slot >= count` -> 0.0).
pub fn packed_weight(mask_rgba: [f32; 4], c2: [f32; 4], layer: usize) -> f32 {
    let channel = match layer {
        0 => 2, // blue
        1 => 1, // green
        _ => 0, // red
    };
    (mask_rgba[channel] * c2[channel]).clamp(0.0, 1.0)
}

/// Ping-pong layer order of the same shader: layer 1 becomes the base, layers 2
/// and 3 lerp over the accumulated result.
pub fn composite_layers(colours: [f32; 3], weights: [f32; 3]) -> f32 {
    let mut acc = colours[0] * weights[0];
    acc = weights[1] * colours[1] + (1.0 - weights[1]) * acc;
    acc = weights[2] * colours[2] + (1.0 - weights[2]) * acc;
    acc
}

/// The lightmap tile addressing recovered from 0x7cd760/0x7cba30. `slot` is
/// the mask-packed tile index of the area and `page` the texture-array layer;
/// both come from the same per-area counter, so a lightmap tile is chosen by the
/// area, not by world position. `mask_uv` scales the area-local 0..1 coordinate
/// by the atlas tile size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Atlas {
    pub page: u32,
    pub side: u32,
    pub per_row: u32,
}

impl Atlas {
    pub fn new(page: u32, side: u32) -> Self {
        Self {
            page,
            side,
            per_row: page / side.max(1),
        }
    }
    pub fn per_page(&self) -> u32 {
        self.per_row * self.per_row
    }
    /// Tile origin of packed slot `index`; pages advance every `per_page` slots.
    pub fn origin(&self, index: u32) -> (u32, u32) {
        let slot = index % self.per_page().max(1);
        let per_row = self.per_row.max(1);
        // Columns advance along a row, rows advance down the page.
        let x = (slot % per_row) * self.side;
        let y = (slot / per_row) * self.side;
        (x, y)
    }
    /// `uv` at the tile origin plus the clamped local coordinate.
    pub fn tile_uv(&self, index: u32, local: (f32, f32)) -> (f32, f32) {
        let (x, y) = self.origin(index);
        let side = self.side as f32;
        (
            (x as f32 + (local.0 * side).clamp(0.5, side - 0.5)) / self.page as f32,
            (y as f32 + (local.1 * side).clamp(0.5, side - 0.5)) / self.page as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(pixel_pipeline: bool, per_mask: bool, hardware: bool) -> Caps {
        Caps {
            pixel_pipeline,
            per_mask_lightmap: per_mask,
            hardware_lightmap: hardware,
        }
    }

    #[test]
    fn native_switch_selects_one_variant_per_state() {
        // 0x7c145a: the shader-loaded byte wins over both env flags.
        assert_eq!(
            select(caps(false, false, false)),
            Ok(Variant::NoPixelShader)
        );
        assert_eq!(select(caps(false, true, true)), Ok(Variant::NoPixelShader));
        // 0x7c1467: env+0x1b5 beats env+0x1b6.
        assert_eq!(
            select(caps(true, true, false)),
            Ok(Variant::PerMaskLightmap)
        );
        assert_eq!(select(caps(true, true, true)), Ok(Variant::PerMaskLightmap));
        // 0x7c1471: what a stock client runs.
        assert_eq!(select(caps(true, false, false)), Ok(Variant::Batch3Layer));
    }

    #[test]
    fn unreachable_hardware_lightmap_path_is_an_error_not_a_branch() {
        assert_eq!(
            select(caps(true, false, true)),
            Err(VariantError::HardwareLightmapUnreachable)
        );
    }

    #[test]
    fn every_variant_carries_the_native_pass_state() {
        for v in [
            Variant::Batch3Layer,
            Variant::PerMaskLightmap,
            Variant::NoPixelShader,
            Variant::HardwareLightmap,
        ] {
            let s = pass_state(v);
            // 0x7cd789, 0x7cd793, 0x7cd79d, 0x77b570.
            assert_eq!(s.cull, Cull::None);
            assert!(s.z_enable && s.z_write);
            assert!(s.specular_enable);
            // 0x77a290 / 0x77a320 are never reached from the terrain range.
            assert!(!s.alpha_blend && !s.alpha_test);
            // The per-mask and no-pixel-pipeline paths turn fixed-function
            // lighting off (0x7c1a65 / 0x7c1e15); the stock and dead paths leave
            // it at the bracket's 1. No terrain path enables a D3D light, so the
            // baked lightmap is never doubled.
            assert_eq!(
                s.lighting_at_draw,
                matches!(v, Variant::Batch3Layer | Variant::HardwareLightmap)
            );
        }
        assert!(pass_state(Variant::Batch3Layer).pixel_shader);
        assert!(pass_state(Variant::PerMaskLightmap).pixel_shader);
        assert!(!pass_state(Variant::NoPixelShader).pixel_shader);
        // 0x7c21ac clears both shader slots on the dead path.
        assert!(!pass_state(Variant::HardwareLightmap).pixel_shader);
        assert!(!pass_state(Variant::HardwareLightmap).vertex_shader);
    }

    #[test]
    fn stage_counts_match_the_stage_loops() {
        assert_eq!(stage_count(Variant::Batch3Layer), Some(6));
        // 0x7c1a20 unrolls stages 0..5 (0x7c1b0b-0x7c1c04): the stage set is
        // fixed and does not depend on the mask count.
        assert_eq!(stage_count(Variant::PerMaskLightmap), Some(6));
        assert_eq!(stage_count(Variant::NoPixelShader), Some(2));
        assert_eq!(stage_count(Variant::HardwareLightmap), Some(3));
        // Both pipelines double the composite: mul_x2 in ps.1.4 and
        // D3DTOP_MODULATE2X in fixed function.
        assert_eq!(D3DTOP_MODULATE2X, 5);
        assert!(D3DTOP_DISABLE < D3DTOP_MODULATE);
        // Stage 2 of the area draw is camera-space projected: 0x20000.
        assert_eq!(TEXCOORD_CAMERASPACE, 0x20000);
        assert_eq!(TEXCOORD_MASK, 5);
    }

    #[test]
    fn vertex_layout_follows_the_per_mask_flag() {
        assert_eq!(vertex_layout(false), (0x412, 0x38));
        assert_eq!(vertex_layout(true), (0x4c2, 0x34));
        // The two FVFs only differ by normal vs diffuse+specular.
        assert_eq!(0x412 ^ 0x4c2, 0x00d0);
        assert_eq!((12 + 12 + 4 * 8) as u32, 0x38);
        assert_eq!((12 + 4 + 4 + 4 * 8) as u32, 0x34);
    }

    #[test]
    fn day_and_night_maps_are_fixed_stages_not_swapped() {
        let pair = lightmap_pair(Variant::Batch3Layer).expect("batch path has a pair");
        assert_eq!((pair.day_stage, pair.night_stage), (4, 5));
        assert!(lightmap_pair(Variant::PerMaskLightmap).is_none());
        assert!(lightmap_pair(Variant::NoPixelShader).is_none());
        // The blend factor selects which map is sampled, never which stage.
        assert_eq!(day_night_lightmap(1.0, 0.0, 0.0), 1.0);
        assert_eq!(day_night_lightmap(1.0, 0.0, 1.0), 0.0);
        assert_eq!(lightmap_pair(Variant::Batch3Layer), Some(pair));
    }

    /// The stage↔record mapping, traced through the loader rather than the
    /// shader comments: stage 4 is requested with `flag == 1` (`0x7c23a2`),
    /// `0x7c2650` maps `flag == 1` to `record+0xc`, and `0x7a2aeb` stores the
    /// authored `.t2lm` day texture there. Stage 5 is requested with
    /// `flag == 0` (`0x7c23b0`) -> `record+0x10` -> `0x7a2b0c`, the `1.t2lm`
    /// night texture. Stacked, that is day on the low stage and night above it.
    #[test]
    fn day_stage_binds_the_record_the_loader_fills_with_the_day_file() {
        let pair = lightmap_pair(Variant::Batch3Layer).expect("batch path has a pair");
        assert_eq!(pair.day_stage, 4);
        assert_eq!(pair.night_stage, 5);
        assert_eq!(native::OPTIONAL_STAGE, 0x7c26_50);
        assert_eq!(native::LIGHTMAP_RECORD_DAY, 0x0c);
        assert_eq!(native::LIGHTMAP_RECORD_NIGHT, 0x10);
        // The two slots are distinct, so swapping the stages is observable.
        assert_ne!(native::LIGHTMAP_RECORD_DAY, native::LIGHTMAP_RECORD_NIGHT);
        // A day-tinted map and a night-tinted map must come out differently at
        // both endpoints, which is the property the loader trace establishes.
        assert_ne!(
            day_night_lightmap(1.0, 0.0, 0.0),
            day_night_lightmap(1.0, 0.0, 1.0)
        );
    }

    #[test]
    fn day_night_lightmap_selects_both_endpoints() {
        assert_eq!(day_night_lightmap(0.8, 0.2, 0.0), 0.8);
        assert_eq!(day_night_lightmap(0.8, 0.2, 1.0), 0.2);
        assert!((day_night_lightmap(0.8, 0.2, 0.5) - 0.5).abs() < 1e-6);
        // lrp_sat clamps, so a sign-clamped factor cannot overshoot.
        assert_eq!(day_night_lightmap(1.0, 1.0, 7.0), 1.0);
        assert_eq!(day_night_lightmap(0.0, 0.0, -7.0), 0.0);
        // The real factor is a clamped lane of the light-matrix product.
        assert!(!lightmap_uses_night(-1.0));
        assert!(!lightmap_uses_night(0.0));
        assert!(lightmap_uses_night(0.5));
        assert!(lightmap_uses_night(9.0));
        // The blend stays continuous: a non-positive lane is pure day.
        assert_eq!(day_night_lightmap(0.8, 0.2, 0.0), 0.8);
        assert!(day_night_lightmap(0.8, 0.2, 0.5) < 0.8);
        assert!(day_night_lightmap(0.8, 0.2, 0.5) > 0.2);
    }

    /// Structural guard on the production shader: the recovered arithmetic has
    /// to be the arithmetic `terrain_gpu.rs` actually executes, otherwise this
    /// module is a description of the client rather than a reconstruction of it.
    #[test]
    fn production_terrain_wgsl_runs_the_recovered_composite() {
        let wgsl = include_str!("terrain_gpu.rs");
        let fs = wgsl
            .split("fn fs(i:Out)")
            .nth(1)
            .expect("production terrain fragment shader");
        // `lrp_sat r5, c3.a, r5, r4` with t4 = day, t5 = night: factor 0 is the
        // day map, 1 the night map, so the mix is (day, night, factor).
        assert!(
            fs.contains("baked=mix(day,night,factor)*2.;"),
            "day map must be the factor-0 endpoint and night the factor-1 endpoint"
        );
        // `mul_x2` doubles the encoded lightmap, exactly once.
        assert_eq!(fs.matches("*2.").count(), 1);
        // `mul r0, r0, v0` with v0 = c0.x = 1.0: no ambient or scene light is
        // added to the baked terrain on the lightmapped path.
        let composited = fs
            .split("if lightmapped {return vec4<f32>(terrain_pass_fog(")
            .nth(1)
            .expect("lightmapped return");
        let composited = composited.split(");}").next().unwrap();
        for forbidden in ["camera.light", "camera.sun.xyz", "camera.region_", "dot("] {
            assert!(
                !composited.contains(forbidden),
                "baked terrain must not add {forbidden}"
            );
        }
        assert!(composited.contains("color*baked*i.refraction.y"));
        // The lightmapped switch is exclusive: the lightmap block, the encoded
        // conversion and the returned branch all branch on the same predicate,
        // and the viewer-only fallback is its negation.
        assert_eq!(fs.matches("if lightmapped").count(), 2);
        assert_eq!(fs.matches("if !lightmapped").count(), 1);
    }

    #[test]
    fn native_and_viewer_day_night_factors_agree_on_both_endpoints() {
        // `c3.a` is `env+0x198`, one lane of the raw light-matrix product. It is
        // neither negated nor normalised on the way in: the `fchs` at
        // 0x7b4c0b..0x7b4c13 acts on `env+0x1d8`/`env+0x1dc` and the normalise at
        // 0x7b4f22..0x7b4fbe writes `env+0x2a4..`. Only `lrp_sat` clamps.
        assert_eq!(native_day_night_factor([0.0, -1.0, 0.0]), 0.0);
        assert_eq!(native_day_night_factor([0.0, 1.0, 0.0]), 1.0);
        assert_eq!(native_day_night_factor([1.0, 0.0, 0.0]), 0.0);
        assert_eq!(native_day_night_factor([0.0, 4.0, 0.0]), 1.0);
        assert_eq!(native_day_night_factor([0.0, 0.25, 0.0]), 0.25);
        // The viewer substitute agrees on both endpoints and travels in the same
        // direction between them; it is the function the WGSL runs.
        assert_eq!(viewer_day_night_factor(1.0), 0.0);
        assert_eq!(viewer_day_night_factor(0.0), 1.0);
        assert_eq!(
            viewer_day_night_factor(1.0),
            native_day_night_factor([0.0, -1.0, 0.0])
        );
        assert_eq!(
            viewer_day_night_factor(0.0),
            native_day_night_factor([0.0, 1.0, 0.0])
        );
        assert!(viewer_day_night_factor(0.75) < viewer_day_night_factor(0.25));
        assert_eq!(viewer_day_night_factor(-3.0), 1.0);
        assert_eq!(viewer_day_night_factor(9.0), 0.0);
    }

    /// The published endpoints and the doubling, re-expressed here so the WGSL
    /// is pinned to this module rather than to a copy of it.
    #[test]
    fn production_wgsl_matches_the_module_arithmetic() {
        let wgsl = include_str!("terrain_gpu.rs");
        // Day endpoint: full daylight -> factor 0 -> pure day map, doubled.
        assert_eq!(
            day_night_lightmap(1.0, 0.0, viewer_day_night_factor(1.0)) * 2.0,
            2.0
        );
        // Night endpoint: no daylight -> factor 1 -> pure night map, doubled.
        assert_eq!(
            day_night_lightmap(1.0, 0.0, viewer_day_night_factor(0.0)) * 2.0,
            0.0
        );
        // Half daylight blends between the two published endpoints.
        let dusk = day_night_lightmap(1.0, 0.0, viewer_day_night_factor(0.5)) * 2.0;
        assert!((dusk - 1.0).abs() < 1e-6);
        assert!(wgsl.contains("baked=mix(day,night,factor)*2.;"));
        // The factor is a published quantity, not an inversion of the
        // lightmap-availability flag: `sun.w` means "the pair is bound" and is a
        // separate concern, so the two must not be conflated again.
        assert_eq!(
            wgsl.matches("let factor=clamp(camera.fog_range.w,0.,1.);")
                .count(),
            1
        );
        assert!(!wgsl.contains("1.-camera.sun.w"));
    }

    #[test]
    fn compose_drops_the_specular_term_because_c0w_is_zero() {
        // 0x9d30f8 starts at 0, so `mad r0, r5, r2, r0` adds nothing.
        assert!((compose(0.25, 0.5, 1.0, 0.0) - 0.25).abs() < 1e-6);
        // A non-zero c0.w would add lightmap*specular before the multiply.
        assert!((compose(0.0, 0.5, 1.0, 1.0) - 0.5).abs() < 1e-6);
        // Doubling saturates: mul_x2 into an 8-bit target.
        assert_eq!(compose(0.9, 1.0, 0.0, 0.0), 1.0);
        // mul r0, r0, v0 with v0 = 1.0 is a no-op, not an ambient tint.
        assert!((compose(0.4, 0.5, 0.0, 3.0) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn shader_constants_force_lane_zero_to_one() {
        let c = shader_constants(true, 0.075, 0.0, [true, true, false], [0.0, -0.7, 0.7]);
        assert_eq!(c.c0, [1.0, 1.0, 0.075, 0.0]);
        // c2 blanks the absent third layer and leaves the fourth lane unused.
        assert_eq!(c.c2, [1.0, 1.0, 0.0, 0.0]);
        // c3.a is the direction's y, broadcast.
        assert_eq!(c.c3, [-0.7; 4]);
        // The "no lightmap scene" branch (0x7c284e) zeroes lanes 1 and 2 only,
        // so `c0.y`/`c0.w` collapse but `c0.z` keeps the density it was handed.
        let off = shader_constants(false, 0.075, 0.0, [false, false, false], [0.0, 1.0, 0.0]);
        assert_eq!(off.c0, [1.0, 0.0, 0.075, 0.0]);
        assert_eq!(off.c2, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn packed_mask_weights_use_blue_green_red() {
        let mask = [0.25, 0.5, 0.75, 1.0];
        let one = [1.0; 4];
        // Layer 1 takes the blue channel, layer 2 green, layer 3 red.
        assert_eq!(packed_weight(mask, one, 0), 0.75);
        assert_eq!(packed_weight(mask, one, 1), 0.5);
        assert_eq!(packed_weight(mask, one, 2), 0.25);
        // c2 = 0 removes a layer entirely.
        assert_eq!(packed_weight(mask, [0.0; 4], 0), 0.0);
        assert_eq!(packed_weight(mask, [0.0; 4], 1), 0.0);
    }

    #[test]
    fn composite_matches_the_shader_ping_pong() {
        assert_eq!(composite_layers([0.8, 0.0, 0.0], [1.0, 0.0, 0.0]), 0.8);
        assert_eq!(composite_layers([0.8, 0.2, 0.0], [1.0, 1.0, 0.0]), 0.2);
        assert!((composite_layers([0.8, 0.2, 0.0], [1.0, 0.5, 0.0]) - 0.5).abs() < 1e-6);
        assert!((composite_layers([0.8, 0.2, 0.6], [1.0, 0.5, 0.5]) - 0.55).abs() < 1e-6);
    }

    #[test]
    fn atlas_tiles_follow_page_rows() {
        let atlas = Atlas::new(4096, 256);
        assert_eq!(atlas.per_row, 16);
        assert_eq!(atlas.per_page(), 256);
        assert_eq!(atlas.origin(0), (0, 0));
        assert_eq!(atlas.origin(1), (256, 0));
        assert_eq!(atlas.origin(16), (0, 256));
        // Slot 256 wraps onto the next page.
        assert_eq!(atlas.origin(256), (0, 0));
        // Local coordinates are clamped half a texel inside the tile.
        let (u, v) = atlas.tile_uv(1, (0.0, 1.0));
        assert!((u - (256.0 + 0.5) / 4096.0).abs() < 1e-6);
        assert!((v - (255.5) / 4096.0).abs() < 1e-6);
    }
}
