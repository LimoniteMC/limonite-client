// The lines around the block aimed at (`outline.rs`).

struct Frame {
    // Rotation and projection only: positions arrive camera-relative.
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

@vertex
fn vertex_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return frame.view_projection * vec4<f32>(position, 1.0);
}

// Black. Blending happens in linear light here, and vanilla's lines take 40% off
// a pixel's encoded value: 1 - 0.6^2.2 of its light.
@fragment
fn fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.675);
}
