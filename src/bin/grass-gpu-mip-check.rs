//! Synthetic GPU regression for independently filtered grass atlas tiles.
#![allow(dead_code)]
#[path = "../foliage_mips.rs"]
mod foliage_mips;
#[path = "../grass_mips.rs"]
mod grass_mips;
#[path = "../texture_atlas.rs"]
mod texture_atlas;
fn main() {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    println!("GPU {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    // Validate the exact world WGSL including the grass mip-aware sampling path.
    let code = include_str!("../gpu.rs");
    let shader = code
        .split("const SHADER: &str = r#\"")
        .nth(1)
        .unwrap()
        .split("\"#;")
        .next()
        .unwrap();
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("world shader validation"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let side = 256usize;
    let image = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grass mip test"),
        size: wgpu::Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 8,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let slots = [
        (
            texture_atlas::Slot {
                x: 0,
                y: 0,
                w: 128,
                h: 128,
            },
            80,
        ),
        (
            texture_atlas::Slot {
                x: 128,
                y: 0,
                w: 128,
                h: 128,
            },
            160,
        ),
    ];
    for swapped in [false, true] {
        let mut src = vec![0u8; side * side * 4];
        for y in 0..128 {
            for x in 0..256 {
                let o = (y * side + x) * 4;
                let is_red = (x < 128) ^ swapped;
                src[o] = if is_red { 255 } else { 0 };
                src[o + 1] = if is_red { 0 } else { 255 };
                src[o + 3] = if (x + y) % 2 == 0 { 255 } else { 0 };
            }
        }
        let tiles = grass_mips::prepare(&src, &slots, side);
        grass_mips::upload(&queue, &image, &src, &tiles, side);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 7 * 512,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        for level in 1..8 {
            let width = (side >> level) as u32;
            enc.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture: &image,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: ((level - 1) * 512) as u64,
                        bytes_per_row: Some(512),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        queue.submit(Some(enc.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let bytes = slice.get_mapped_range();
        for level in 1..8 {
            let off = (level - 1) * 512;
            let right = off + (128 >> level) * 4;
            let left = &bytes[off..off + 4];
            let right = &bytes[right..right + 4];
            if swapped {
                assert_eq!(left[0..2], [0, 255]);
                assert_eq!(right[0..2], [255, 0]);
            } else {
                assert_eq!(left[0..2], [255, 0]);
                assert_eq!(right[0..2], [0, 255]);
            }
            assert!(
                left[3] >= 80 && right[3] >= 160,
                "coverage below reference level {level}: {left:?} {right:?}"
            );
        }
        drop(bytes);
        buffer.unmap();
    }
    println!(
        "Passed: seven isolated mip levels, distinct alpha references, re-upload after repacking"
    );
}
