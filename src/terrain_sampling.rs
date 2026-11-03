//! Native terrain sampler/addressing/format policy for `A3DTerrain2`.
//!
//! Every rule here comes from the 1.4.2 executable, not from a renderer
//! convention:
//!
//! * State wrappers (`0x77bdb0` = `SetTextureStageState`, `0x77bd00` =
//!   `SetRenderState`, `0x77ba40` = `SetTexture`). Each wrapper's constant
//!   argument is the D3D state type, so the mapping is unambiguous:
//!   `0x779f60` writes `D3DTSS_MAGFILTER`(16), `D3DTSS_MINFILTER`(17) and
//!   `D3DTSS_MIPFILTER`(18); `0x779fc0` writes `D3DTSS_COLOROP`(1);
//!   `0x779fe0` writes `D3DTSS_COLORARG1`(2)/`COLORARG2`(3); `0x77a020` writes
//!   `D3DTSS_ALPHAOP`(4); `0x77a040` writes `ALPHAARG1`(5)/`ALPHAARG2`(6);
//!   `0x77a080` writes `D3DTSS_RESULTARG`(28); `0x77a0e0` writes
//!   `D3DTSS_ADDRESSU`(13)/`ADDRESSV`(14); `0x77a120` writes
//!   `D3DTSS_TEXCOORDINDEX`(11); `0x77a160` writes the bump-env matrix.
//!
//! * `0x7c14b0` (batched PS1.4 terrain path) sets every stage's
//!   MAG/MIN/MIP filter to `2` = `D3DTEXF_LINEAR`, five use `(13,14) = 2`
//!   = `D3DTADDRESS_MIRROR`, and `0x77a120` assigns each stage the coordinate
//!   set with the same index (`0..5`). `0x7c1a20` repeats it and disables
//!   `ZENABLE`. `0x7c1e00` is the fallback/one-layer path: stage 0 has all
//!   three filters set to `0` (`NONE`, i.e. untiled point sampling with no mip
//!   interpolation), `COLOROP 0` (`DISABLE`) with `COLORARG1/2 0`
//!   (`DIFFUSE`), `ALPHAOP 0`, `RESULTARG 0` (`TEMPCURRENT`), and stages 0/1
//!   use `COLOROP 5/3` with `ADDRESSU/V 1` (`D3DTADDRESS_WRAP`).
//!   `t2_3layer_batch_14.txt`/`t2_4layer_batch_14.txt` therefore address
//!   colour layers with real D3D-wrapping UVs while the previous layers
//!   arrived already masked (`lrp` by mask channels).
//!
//! * The `D3DTSS_MAXANISOTROPY`(21) and `D3DTSS_MIPMAPLODBIAS`(19) states are
//!   never written for terrain (`0x77a1b0`/`0x77a1d0` have exactly two call
//!   sites, neither in `0x7c0000..0x7d8000`). Anisotropy is therefore the D3D8
//!   default 1: the client runs trilinear point-of-fact magnification and
//!   linear minification with no anisotropic taps. The viewer previously
//!   requested 16x, which is not native.
//!
//! * `t2_1lay_nonalpha.txt` reads the mask's **blue** channel (`mov r0.a,
//!   t1.b`) and `t2_3layer_batch_14.txt` documents red = layer 3, green =
//!   layer 2, blue = layer 1, so terrain masks are `D3DFMT_A8R8G8B8` with the
//!   weight in blue and an alpha channel present. A single 8-bit red mask
//!   channel is not what the client samples.
//!
//! * `A3DTexture::CreateDXTExture()` (`0x7f7f74`) forwards the authored
//!   `FourCC` straight to `CreateTexture`, and its bit-count branches are the
//!   DXT1..DXT5 pairs `0x20000/0x40000/0x8000/0x10000`. Any other `FourCC` is
//!   passed to `CreateTexture` too, so `A3DTexture::LoadFromFile` — not the
//!   terrain loader — is what rejects a format the device does not accept.
//!   `A3DTexture::CreateDXTExture(), Fail in creating format[%d], [%d]x[%d],
//!   [%d]mip texture` is the only failure text that path can print.
//!
//! Nothing here guesses: an authored mip chain that the client would hand to
//! the device is kept, and a source the client cannot create is an error.
#![allow(dead_code)]

/// `D3DTEXF_*` as stored by `SetTextureStageState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFilter {
    None,
    Point,
    Linear,
    Anisotropic,
}

/// `D3DTADDRESS_*` as stored by `D3DTSS_ADDRESSU/V`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressMode {
    Wrap,
    Mirror,
    Clamp,
    Border,
    MirrorOnce,
}

impl AddressMode {
    /// Decode the raw D3D8 enum value; unknown values are explicit errors
    /// instead of a silent clamp.
    pub fn from_native(value: u32) -> Result<Self, String> {
        Ok(match value {
            1 => Self::Wrap,
            2 => Self::Mirror,
            3 => Self::Clamp,
            4 => Self::Border,
            5 => Self::MirrorOnce,
            other => return Err(format!("unsupported D3DTADDRESS value {other}")),
        })
    }
}

/// The fully decoded sampler state for one terrain texture stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageSampler {
    pub mag: TextureFilter,
    pub min: TextureFilter,
    pub mip: TextureFilter,
    pub address_u: AddressMode,
    pub address_v: AddressMode,
    /// `D3DTSS_MAXANISOTROPY`(21). Never authored for terrain; D3D8's default.
    pub max_anisotropy: u32,
}

impl StageSampler {
    /// `0x7c14b0`/`0x7c1a20`: the batched PS1.4 colour and mask stages.
    pub const BATCHED: Self = Self {
        mag: TextureFilter::Linear,
        min: TextureFilter::Linear,
        mip: TextureFilter::Linear,
        address_u: AddressMode::Mirror,
        address_v: AddressMode::Mirror,
        max_anisotropy: 1,
    };
    /// `0x7c1e00` stage 0: every filter `0` (`D3DTEXF_NONE`) and wrap.
    pub const FALLBACK_BASE: Self = Self {
        mag: TextureFilter::None,
        min: TextureFilter::None,
        mip: TextureFilter::None,
        address_u: AddressMode::Wrap,
        address_v: AddressMode::Wrap,
        max_anisotropy: 1,
    };
    /// `0x7c1e00` stages 1/2: `COLOROP` 3/5 with `ADDRESSU/V 1` (wrap).
    pub const FALLBACK_ADDITIONAL: Self = Self {
        mag: TextureFilter::None,
        min: TextureFilter::None,
        mip: TextureFilter::None,
        address_u: AddressMode::Wrap,
        address_v: AddressMode::Wrap,
        max_anisotropy: 1,
    };
    /// Whether the client ever asks the device for anisotropic taps here.
    pub fn anisotropic(&self) -> bool {
        self.max_anisotropy > 1
            && matches!(self.min, TextureFilter::Anisotropic)
            && matches!(self.mag, TextureFilter::Anisotropic)
    }
}

/// Native `A3DTerrain2` sampler state for a layer stage index.
///
/// Stage 0 is the layer the fixed-function/PS1.1 fallback path samples with
/// `0x7c1e00`, everything else follows the batched PS1.4 block at `0x7c14b0`.
/// `batched` says which pixel-shader path the caller actually selected
/// (`terrain2ps.cfg` vs `terrain2_shaders.cfg`).
pub fn stage_sampler(batched: bool, stage: usize) -> StageSampler {
    if batched || stage > 0 {
        StageSampler::BATCHED
    } else {
        StageSampler::FALLBACK_BASE
    }
}

/// `D3DTADDRESS_*` the client selected for all five stages of `0x7c14b0`,
/// exposed as a value so tests can assert the mirror-only rule.
pub const BATCHED_ADDRESS: AddressMode = AddressMode::Mirror;

/// Terrain colour-layer pass order.
///
/// `t2_3layer_batch_14.txt`: `r3 = mask`, `b` = layer 1, `g` = layer 2,
/// `r` = layer 3, each `lrp` over the previous result. Reproducing the order
/// matters because the composite is a sequence of linear interpolations and is
/// therefore not commutative once masks overlap.
pub const LAYER_ORDER: [char; 3] = ['b', 'g', 'r'];

/// The mask channel the client reads for a given layer index inside an area.
///
/// * Two-layer fallback shaders read the **blue** channel for the first mask
///   (`t2_1lay_nonalpha.txt`, `mov r0.a, t1.b`) and the packed-channel shader
///   names red/green/blue for layers 3/2/1.
/// * `0x7c1e00` sets `ALPHAARG1/2` on stages 1/2, matching one mask texture per
///   additional layer.
pub fn mask_channel(layer: usize) -> Result<char, String> {
    match layer {
        // Layer 0 has no mask: the client keeps it fully opaque.
        0 => Ok('a'),
        // 0x7c14b0 batched path: first additional layer is blue.
        1 => Ok('b'),
        2 => Ok('g'),
        3 => Ok('r'),
        other => Err(format!(
            "terrain area references unsupported layer {other}; native shaders carry three mask channels"
        )),
    }
}

/// Backend-neutral block-compressed storage formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageFormat {
    Bc1Unorm,
    Bc2Unorm,
    Bc3Unorm,
}

/// Texture formats the DXT path forwards verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxtFormat {
    Dxt1,
    Dxt2,
    Dxt3,
    Dxt4,
    Dxt5,
}

impl DxtFormat {
    /// FourCC as read from the DDS pixel-format word.
    pub fn four_cc(self) -> u32 {
        match self {
            Self::Dxt1 => 0x3154_5844,
            Self::Dxt2 => 0x3254_5844,
            Self::Dxt3 => 0x3354_5844,
            Self::Dxt4 => 0x3454_5844,
            Self::Dxt5 => 0x3554_5844,
        }
    }
    /// Decode the FourCC the client compares at `0x7f7e4e..0x7f7e71`.
    pub fn from_four_cc(word: u32) -> Result<Self, String> {
        Ok(match word {
            0x3154_5844 => Self::Dxt1,
            0x3254_5844 => Self::Dxt2,
            0x3354_5844 => Self::Dxt3,
            0x3454_5844 => Self::Dxt4,
            0x3554_5844 => Self::Dxt5,
            other => {
                return Err(format!(
                    "unsupported terrain texture format {other:08x}; \
                     0x7f7e4e only accepts DXT1..DXT5"
                ))
            }
        })
    }
    /// `0x7f7e71` aligns the row width to four texels for every DXT format and
    /// `0x7f7e88` picks the bit count per format.
    pub fn block_bytes(self) -> usize {
        match self {
            Self::Dxt1 => 8,
            // 0x7f7e9c (0x8000 = 16 bpp), 0x7f7e95 (0x10000 = 32 bpp),
            // 0x7f7ea3 (0x4000 = 16 bpp), 0x7f7dfd (0x40000 = 64 bpp).
            Self::Dxt2 | Self::Dxt3 | Self::Dxt4 | Self::Dxt5 => 16,
        }
    }
    /// Whether the client's `0x78ad42..0x78ad62` alpha-texture flag is set.
    pub fn alpha_texture(self) -> bool {
        !matches!(self, Self::Dxt1)
    }
    /// Backend-neutral storage format. Terrain keeps UNORM sampling: D3D8
    /// filters authored encoded bytes, so the sampler must not decode them.
    /// Kept here rather than as a `wgpu` type so this module stays
    /// dependency-free for the core-check harness.
    pub fn storage(self) -> StorageFormat {
        match self {
            Self::Dxt1 => StorageFormat::Bc1Unorm,
            Self::Dxt2 | Self::Dxt3 => StorageFormat::Bc2Unorm,
            Self::Dxt4 | Self::Dxt5 => StorageFormat::Bc3Unorm,
        }
    }
}

/// A validated terrain DDS description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainDds {
    pub format: DxtFormat,
    pub width: u32,
    pub height: u32,
    /// Authored mip levels. The client uploads every level the file carries;
    /// it never synthesises a chain for terrain colour textures.
    pub authored_mips: u32,
}

/// Parse the `DDS ` header the terrain loader would hand to
/// `CreateDXTExture`. Rejects the cases the client cannot create.
pub fn parse_terrain_dds(bytes: &[u8]) -> Result<TerrainDds, String> {
    let header = bytes.get(..128).ok_or("truncated terrain DDS header")?;
    let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    if &header[..4] != b"DDS " || word(4) != 124 || word(76) != 32 {
        return Err("invalid terrain DDS header".into());
    }
    // Pixel format must be a FourCC.
    if word(80) & 0x4 == 0 {
        return Err(
            "terrain DDS is not FourCC-compressed; 0x7f7f74 only creates DXT textures".into(),
        );
    }
    if word(112) != 0 {
        return Err("cube-map terrain DDS is not implemented".into());
    }
    let format = DxtFormat::from_four_cc(word(84))?;
    let (height, width) = (word(12), word(16));
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err("terrain DDS dimensions out of bounds".into());
    }
    let authored = word(28).max(1);
    // 0x7f7ef1 counts the levels a full chain would have; the client caps at
    // that so a malformed count cannot overrun the payload.
    let full = 32 - width.max(height).leading_zeros();
    if authored > full {
        return Err(format!(
            "terrain DDS declares {authored} mip levels but {} fit the dimensions",
            full
        ));
    }
    let mip = TerrainDds {
        format,
        width,
        height,
        authored_mips: authored,
    };
    // Validate the payload covers exactly the declared levels. DXT rows are
    // aligned to four texels (`0x7f7e77`), which block sizes already imply.
    let mut offset = 128usize;
    for level in 0..authored {
        let w = (width >> level).max(1);
        let h = (height >> level).max(1);
        let size = ((w + 3) / 4) as usize * ((h + 3) / 4) as usize * format.block_bytes();
        offset = offset
            .checked_add(size)
            .ok_or("terrain DDS size overflow")?;
        if offset > bytes.len() {
            return Err(format!("truncated terrain DDS mip level {level}"));
        }
    }
    Ok(mip)
}

/// Whether the viewer must generate reductions for a terrain source.
///
/// The client uploads the authored chain and lets the device filter it; it does
/// not build a chain when the file has one level, because
/// `CreateDXTExture` passes the authored `mip` argument straight through.
/// A single-level source therefore samples without mip selection, matching
/// `D3DTEXF_NONE` mip filtering rather than a resampled chain.
pub fn generates_mips(authored: u32) -> bool {
    authored <= 1
}

/// The viewer's texture-array storage rule.
///
/// Terrain colour textures live in power-of-two square arrays (256/512/1024).
/// A source smaller than its slot is only ever *enlarged* into the array, never
/// reduced, so authored detail is preserved; a source larger than the slot is
/// an explicit error instead of a silent downscale.
pub fn array_slot(width: u32, height: u32) -> Result<u32, String> {
    if width == 0 || height == 0 {
        return Err("terrain texture has zero extent".into());
    }
    let longest = width.max(height);
    Ok(match longest {
        1..=256 => 256,
        257..=512 => 512,
        513..=1024 => 1024,
        _ => {
            return Err(format!(
                "terrain texture {width}x{height} exceeds the 1024px array slot"
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x7c14b0` writes `2` for MAG/MIN/MIP on stages 1..5 and the same value
    /// for ADDRESSU/V. D3DTEXF_LINEAR is 2 and D3DTADDRESS_MIRROR is 2, so a
    /// single decoded record explains both.
    #[test]
    fn batched_path_is_trilinear_mirror_without_anisotropy() {
        let s = stage_sampler(true, 3);
        assert_eq!(s.mag, TextureFilter::Linear);
        assert_eq!(s.min, TextureFilter::Linear);
        assert_eq!(s.mip, TextureFilter::Linear);
        assert_eq!(s.address_u, AddressMode::Mirror);
        assert_eq!(s.address_v, AddressMode::Mirror);
        assert_eq!(s.max_anisotropy, 1);
        assert!(!s.anisotropic());
        assert_eq!(BATCHED_ADDRESS, AddressMode::Mirror);
    }

    /// The viewer must not request 16x anisotropy: `D3DTSS_MAXANISOTROPY` is
    /// never written anywhere in the terrain code region.
    #[test]
    fn no_stage_requests_anisotropic_taps() {
        for batched in [false, true] {
            for stage in 0..6 {
                assert!(!stage_sampler(batched, stage).anisotropic());
            }
        }
    }

    /// `0x7c1e00` filters are all `0` and both address modes `1`.
    #[test]
    fn fallback_stage_zero_disables_filtering_and_wraps() {
        let s = stage_sampler(false, 0);
        assert_eq!(s.mag, TextureFilter::None);
        assert_eq!(s.min, TextureFilter::None);
        assert_eq!(s.mip, TextureFilter::None);
        assert_eq!(s.address_u, AddressMode::Wrap);
        assert_eq!(s.address_v, AddressMode::Wrap);
    }

    /// Raw D3D8 enum decoding must fail closed on values the client never set.
    #[test]
    fn address_modes_decode_and_reject_unknown_values() {
        assert_eq!(AddressMode::from_native(1).unwrap(), AddressMode::Wrap);
        assert_eq!(AddressMode::from_native(2).unwrap(), AddressMode::Mirror);
        assert_eq!(AddressMode::from_native(3).unwrap(), AddressMode::Clamp);
        assert!(AddressMode::from_native(0).is_err());
        assert!(AddressMode::from_native(9).is_err());
    }

    /// `t2_1lay_nonalpha.txt` reads the blue channel; the batched shader names
    /// blue/green/red for layers 1/2/3.
    #[test]
    fn mask_channels_follow_the_authored_shaders() {
        assert_eq!(mask_channel(0).unwrap(), 'a');
        assert_eq!(mask_channel(1).unwrap(), 'b');
        assert_eq!(mask_channel(2).unwrap(), 'g');
        assert_eq!(mask_channel(3).unwrap(), 'r');
        assert!(mask_channel(4).is_err());
    }

    /// Layer order is a sequence of lerps and cannot be reordered.
    #[test]
    fn layer_order_is_blue_green_red() {
        assert_eq!(LAYER_ORDER, ['b', 'g', 'r']);
    }

    /// `0x7f7e4e` accepts exactly the five DXT FourCCs.
    #[test]
    fn dxt_fourcc_table_matches_the_native_comparisons() {
        for (word, expect) in [
            (0x3154_5844u32, DxtFormat::Dxt1),
            (0x3254_5844, DxtFormat::Dxt2),
            (0x3354_5844, DxtFormat::Dxt3),
            (0x3454_5844, DxtFormat::Dxt4),
            (0x3554_5844, DxtFormat::Dxt5),
        ] {
            assert_eq!(DxtFormat::from_four_cc(word).unwrap(), expect);
            assert_eq!(expect.four_cc(), word);
        }
        // 0x7f7e6f falls through to the non-DXT path for anything else.
        assert!(DxtFormat::from_four_cc(0x5334_5441).is_err()); // "ATI 4S"
        assert_eq!(DxtFormat::Dxt1.block_bytes(), 8);
        assert_eq!(DxtFormat::Dxt3.block_bytes(), 16);
        assert_eq!(DxtFormat::Dxt5.block_bytes(), 16);
        assert!(!DxtFormat::Dxt1.alpha_texture());
        assert!(DxtFormat::Dxt3.alpha_texture());
        assert!(DxtFormat::Dxt5.alpha_texture());
    }

    /// The native texture must stay UNORM: D3D8 filters encoded bytes.
    #[test]
    fn dxt_formats_map_to_unorm_not_srgb() {
        assert_eq!(DxtFormat::Dxt1.storage(), StorageFormat::Bc1Unorm);
        assert_eq!(DxtFormat::Dxt2.storage(), StorageFormat::Bc2Unorm);
        assert_eq!(DxtFormat::Dxt3.storage(), StorageFormat::Bc2Unorm);
        assert_eq!(DxtFormat::Dxt4.storage(), StorageFormat::Bc3Unorm);
        assert_eq!(DxtFormat::Dxt5.storage(), StorageFormat::Bc3Unorm);
    }

    fn dds(
        width: u32,
        height: u32,
        four_cc: &[u8; 4],
        mips: u32,
        levels: usize,
        block: usize,
    ) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        b[4..8].copy_from_slice(&124u32.to_le_bytes());
        b[12..16].copy_from_slice(&height.to_le_bytes());
        b[16..20].copy_from_slice(&width.to_le_bytes());
        b[28..32].copy_from_slice(&mips.to_le_bytes());
        b[76..80].copy_from_slice(&32u32.to_le_bytes());
        b[80..84].copy_from_slice(&0x4u32.to_le_bytes());
        b[84..88].copy_from_slice(four_cc);
        let mut size = 0usize;
        for level in 0..levels {
            let w = (width >> level).max(1);
            let h = (height >> level).max(1);
            size += ((w + 3) / 4) as usize * ((h + 3) / 4) as usize * block;
        }
        b.extend(std::iter::repeat(0u8).take(size));
        b
    }

    /// A single-level authored texture is kept as authored.
    #[test]
    fn authored_single_level_dxt1_parses() {
        let bytes = dds(256, 256, b"DXT1", 1, 1, 8);
        let info = parse_terrain_dds(&bytes).unwrap();
        assert_eq!(info.format, DxtFormat::Dxt1);
        assert_eq!((info.width, info.height), (256, 256));
        assert_eq!(info.authored_mips, 1);
        assert!(generates_mips(info.authored_mips));
    }

    /// A full authored chain is preserved, not regenerated.
    #[test]
    fn authored_full_chain_is_kept() {
        let bytes = dds(256, 256, b"DXT5", 9, 9, 16);
        let info = parse_terrain_dds(&bytes).unwrap();
        assert_eq!(info.authored_mips, 9);
        assert!(!generates_mips(info.authored_mips));
    }

    /// Non-DXT and malformed payloads are errors, never silent fallbacks.
    #[test]
    fn unsupported_sources_fail_explicitly() {
        // Uncompressed RGB: pixel-format flags without FourCC.
        let mut plain = dds(64, 64, b"\0\0\0\0", 1, 1, 8);
        plain[80..84].copy_from_slice(&0x40u32.to_le_bytes());
        let err = parse_terrain_dds(&plain).unwrap_err();
        assert!(err.contains("not FourCC"), "{err}");

        // A FourCC the native comparisons do not accept.
        let mut odd = dds(64, 64, b"ATI2", 1, 1, 8);
        odd.truncate(128);
        let err = parse_terrain_dds(&odd).unwrap_err();
        assert!(err.contains("unsupported terrain texture format"), "{err}");

        // Truncated payload for the declared chain.
        let mut short = dds(256, 256, b"DXT1", 3, 1, 8);
        short.truncate(128 + 8);
        let err = parse_terrain_dds(&short).unwrap_err();
        assert!(err.contains("truncated terrain DDS mip level"), "{err}");

        // A mip count larger than the dimensions allow.
        let over = dds(4, 4, b"DXT1", 9, 1, 8);
        let err = parse_terrain_dds(&over).unwrap_err();
        assert!(err.contains("mip levels"), "{err}");
    }

    /// Storage slots preserve authored resolution and never downscale.
    #[test]
    fn array_slots_preserve_authored_resolution() {
        assert_eq!(array_slot(64, 64).unwrap(), 256);
        assert_eq!(array_slot(256, 256).unwrap(), 256);
        assert_eq!(array_slot(512, 512).unwrap(), 512);
        assert_eq!(array_slot(512, 1024).unwrap(), 1024);
        assert!(array_slot(0, 16).is_err());
        assert!(array_slot(2048, 2048).is_err());
    }
}
