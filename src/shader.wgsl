// Renders the simulation texture to the window, with a selective bloom that
// makes emissive materials (fire, lava) glow.
//
// The sim rasterises each cell into an RGBA texture where the *alpha channel is
// a glow flag*: alpha 0 = emissive, alpha 255 = opaque (see
// `Simulation::render_into`). We never alpha-blend the grid, so that channel is
// free to carry the flag. The glow is a classic three-pass screen-space bloom,
// the same trick the JMS55/sandbox project uses:
//
//   1. `fs_blur_h` — sample the scene, keep only the glowing pixels, and box-
//      blur them horizontally into an offscreen "glow" texture.
//   2. `fs_blur_v` — box-blur that glow texture vertically.
//   3. `fs_composite` — draw the crisp scene and add the blurred glow on top,
//      so emitters bleed a soft halo past their outline.
//
// All three share the one fullscreen-triangle vertex shader below.

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// group(0) is the input image(s); group(1) carries the blur step for the two
// blur passes. Not every pipeline binds every entry — each one only declares a
// layout for the bindings its fragment shader actually uses.
@group(0) @binding(0) var tex0: texture_2d<f32>;     // scene (blur/composite)
@group(0) @binding(1) var samp0: sampler;            // nearest, for the scene
@group(0) @binding(2) var tex1: texture_2d<f32>;     // blurred glow (composite)
@group(0) @binding(3) var samp1: sampler;            // linear, for the glow

struct Blur {
    // Per-tap UV offset: (1/width, 0) for the horizontal pass, (0, 1/height)
    // for the vertical one, scaled by how wide we want the halo to spread.
    step: vec2<f32>,
    _pad: vec2<f32>,
};
@group(1) @binding(0) var<uniform> blur: Blur;

// How strongly the blurred glow is added back over the scene.
const GLOW_STRENGTH: f32 = 1.4;
// Box-blur half-width, in taps. The full kernel is 2*RADIUS+1 taps.
const RADIUS: i32 = 4;
const TAPS: f32 = 9.0; // 2*RADIUS + 1

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

// Pass 1: extract the glowing pixels and blur them horizontally. A pixel glows
// iff its alpha is 0 (`step(a, 0.0)` is 1.0 only when a <= 0), so non-emissive
// cells contribute nothing. Sampled with `samp0` (nearest) so the glow mask
// stays exact rather than bleeding off neighbouring opaque cells.
@fragment
fn fs_blur_h(in: VsOut) -> @location(0) vec4<f32> {
    var acc = vec3<f32>(0.0);
    for (var i: i32 = -RADIUS; i <= RADIUS; i++) {
        let uv = in.uv + vec2<f32>(f32(i) * blur.step.x, 0.0);
        let s = textureSample(tex0, samp0, uv);
        acc += s.rgb * step(s.a, 0.0);
    }
    return vec4<f32>(acc / TAPS, 1.0);
}

// Pass 2: blur the (already glow-only) texture vertically. Sampled linearly via
// `samp0` here for a smoother falloff.
@fragment
fn fs_blur_v(in: VsOut) -> @location(0) vec4<f32> {
    var acc = vec3<f32>(0.0);
    for (var i: i32 = -RADIUS; i <= RADIUS; i++) {
        let uv = in.uv + vec2<f32>(0.0, f32(i) * blur.step.y);
        acc += textureSample(tex0, samp0, uv).rgb;
    }
    return vec4<f32>(acc / TAPS, 1.0);
}

// Pass 3: the crisp scene (nearest) plus the blurred glow (linear), added so
// the halo bleeds past each emitter's outline.
@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let scene = textureSample(tex0, samp0, in.uv).rgb;
    let glow = textureSample(tex1, samp1, in.uv).rgb;
    return vec4<f32>(scene + glow * GLOW_STRENGTH, 1.0);
}
