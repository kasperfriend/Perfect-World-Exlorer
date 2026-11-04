//! Compile the production precipitation and cloud WGSL on the active adapter.
//! This catches backend validation errors at construction time, not only Rust
//! syntax errors or shader parser errors.
use std::{fs, path::Path};

fn shader_source(path: &Path) -> String {
    let source = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let marker = "const SHADER: &str = r#\"";
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("{} has no production SHADER constant", path.display()))
        + marker.len();
    let rest = &source[start..];
    let end = rest
        .find("\"#;")
        .unwrap_or_else(|| panic!("{} has an unterminated SHADER constant", path.display()));
    rest[..end].to_owned()
}

fn validate(device: &wgpu::Device, label: &str, source: String) {
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.poll(wgpu::Maintain::Wait);
    if let Some(error) = pollster::block_on(device.pop_error_scope()) {
        panic!("{label}: {error}");
    }
    println!("Validated production WGSL: {label}");
}

fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::all()),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("no GPU adapter available for WGSL validation");
    let (device, _queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Phase 5 cloud/weather WGSL validation"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        },
        None,
    ))
    .expect("request WGSL validation device");
    validate(
        &device,
        "world weather particle shader",
        shader_source(Path::new("src/weather_gpu.rs")),
    );
    validate(
        &device,
        "world cloud surface shader",
        shader_source(Path::new("src/cloud_gpu.rs")),
    );
}
