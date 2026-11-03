// The lines around the block aimed at (`outline.rs`), each a rectangle facing the
// screen: one instance of two triangles.

struct Frame {
    // Rotation and projection only: positions arrive camera-relative.
    view_projection: mat4x4<f32>,
}

struct Sizes {
    // In pixels.
    window: vec2<f32>,
    width: f32,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> sizes: Sizes;

// How much nearer the camera a line is drawn. A point moved straight towards the camera
// stays on the same pixel, so this only changes its depth. It's what lets the side of a
// wide line lie over the floor at a block's foot, where the line itself is just under it.
const PULL: f32 = 0.99;

@vertex
fn vertex_main(
    @builtin(vertex_index) index: u32,
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
) -> @builtin(position) vec4<f32> {
    var a = frame.view_projection * vec4<f32>(start * PULL, 1.0);
    var b = frame.view_projection * vec4<f32>(end * PULL, 1.0);
    // Wholly behind the camera: all of its corners go to one place, so nothing draws.
    if a.z < 0.0 && b.z < 0.0 {
        return vec4<f32>(0.0, 0.0, -1.0, 1.0);
    }
    // A line can run past the camera, and what's behind it has no place on the screen, so
    // it's cut at the near plane. Depth 0 is the near plane.
    if a.z < 0.0 {
        a = mix(a, b, a.z / (a.z - b.z));
    } else if b.z < 0.0 {
        b = mix(b, a, b.z / (b.z - a.z));
    }

    // The triangles are corners 0 1 2 and 3 4 5: which end of the line each corner is at,
    // and which side of it.
    let at_end = index == 2u || index == 3u || index == 5u;
    let at_left = index == 1u || index == 2u || index == 3u;

    // Which way the line runs on the screen, in pixels, so that its width doesn't depend
    // on the window's shape. Seen end on, it runs nowhere, and any way will do.
    let on_screen = (b.xy / b.w - a.xy / a.w) * sizes.window;
    let long = length(on_screen);
    let along = select(vec2<f32>(1.0, 0.0), on_screen / long, long > 0.001);
    let across = vec2<f32>(-along.y, along.x);

    // Half the width to each side, and as much past each end, so that lines close their
    // corners. A pixel is 2 / window of the screen's width of 2, so half a width is
    // width / window. Multiplying by w undoes the divide the GPU does next.
    let outward = across * select(-1.0, 1.0, at_left) + along * select(-1.0, 1.0, at_end);
    let here = select(a, b, at_end);
    return vec4<f32>(here.xy + outward * sizes.width / sizes.window * here.w, here.zw);
}

// Black. Blending happens in linear light here, and vanilla's lines take 40% off
// a pixel's encoded value: 1 - 0.6^2.2 of its light.
@fragment
fn fragment_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.675);
}
