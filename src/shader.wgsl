// Draws the simulation texture across the whole window.
//
// No vertex buffer: the vertex shader emits one oversized triangle that covers
// clip space, and we sample the grid texture per fragment with nearest
// filtering so individual sand grains stay crisp.

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var grid_tex: texture_2d<f32>;
@group(0) @binding(1) var grid_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Fullscreen triangle: (-1,-1), (3,-1), (-1,3).
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = pos[vi];

    var out: VsOut;
    out.clip_pos = vec4<f32>(p, 0.0, 1.0);
    // Map clip space to texture UV. Flip Y so grid row 0 is the top of the
    // screen and gravity (increasing row index) points downward.
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(grid_tex, grid_sampler, in.uv);
}
