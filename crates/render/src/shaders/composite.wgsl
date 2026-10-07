// A slot's scene, resolved, over the frame's target: the same size, so a
// pixel is copied to the same pixel, blended premultiplied.

@group(0) @binding(0) var scene: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(scene, vec2<i32>(position.xy), 0);
}
