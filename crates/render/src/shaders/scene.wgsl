// See `Uniforms` in renderer.rs for what each field holds.
struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    focus: vec4<f32>,
    backward: vec4<f32>,
    viewport: vec4<f32>,
    background_top: vec4<f32>,
    background_bottom: vec4<f32>,
    model: vec4<f32>,
    edge: vec4<f32>,
    grid: vec4<f32>,
    axes: array<vec4<f32>, 3>,
    origin_outline: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// Set by the renderer. True if the target stores output as is, so it must be
// sRGB encoded here, false if the target encodes it.
override ENCODE_SRGB: bool;
// Grid lines fade out within this many view heights of the target. The
// depth range, fitted in scene.rs, covers them.
override GRID_FADE_HEIGHTS: f32;

// The visible height at the target in world units.
fn view_height() -> f32 {
    return 2.0 * u.up.w;
}

// Shading works in linear colour, which the theme's colours arrive in. Every
// fragment shader passes its result through `output`, so the scene looks the
// same on sRGB and non-sRGB targets, apart from blending, which the latter
// does on encoded values.

fn output(color: vec4<f32>) -> vec4<f32> {
    if !ENCODE_SRGB {
        return color;
    }
    let c = clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let encoded = select(
        1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055,
        c * 12.92,
        c <= vec3<f32>(0.0031308),
    );
    return vec4<f32>(encoded, color.a);
}

// --- Fullscreen passes (background, grid) ---

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: FullscreenOut;
    out.position = vec4<f32>(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_background(in: FullscreenOut) -> @location(0) vec4<f32> {
    return output(mix(u.background_bottom, u.background_top, in.ndc.y * 0.5 + 0.5));
}

struct GridOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Anti-aliased line coverage for a grid with the given spacing. `dp` is the
// screen-space derivative of `p`, passed in so spacing may vary per pixel.
fn grid_lines(p: vec2<f32>, dp: vec2<f32>, spacing: f32) -> f32 {
    let coord = p / spacing;
    let width = max(dp / spacing, vec2<f32>(1e-6));
    let line = abs(fract(coord - 0.5) - 0.5) / width;
    return 1.0 - min(min(line.x, line.y), 1.0);
}

// Opacity of grid level `x`, where `x` counts powers of ten above the level
// currently fading out. Continuous in `x`, so levels hand over seamlessly.
fn level_weight(x: f32) -> f32 {
    return clamp((x + 1.0) * 0.25, 0.0, 0.6);
}

// Infinite grid on the XY plane, ray traced per pixel. The spacing adapts to
// zoom: minor lines are kept at least MIN_PIXELS logical pixels apart and blend into the next
// power of ten as they get denser.
@fragment
fn fs_grid(in: FullscreenOut) -> GridOut {
    let MIN_PIXELS = 16.0;

    // The ray through the pixel, from where it crosses the view plane at the
    // target. Built from the camera basis rather than by unprojecting the
    // near and far planes, which are fitted to the whole scene and would
    // lose the ground to rounding when zoomed into a large one.
    let origin = u.focus.xyz + u.right.xyz * (in.ndc.x * u.right.w)
        + u.up.xyz * (in.ndc.y * u.up.w);
    let dir = origin * u.eye.w - u.eye.xyz;
    let t = -origin.z / dir.z;

    let hit = origin + t * dir;
    let clip = u.view_proj * vec4<f32>(hit, 1.0);
    let depth = clip.z / clip.w;

    // Compute everything unconditionally so derivatives stay well defined.
    let dp = fwidth(hit.xy);
    let pixel = max(length(dp), 1e-6);
    let level = log(pixel * MIN_PIXELS * u.viewport.z) / log(10.0);
    let spacing = pow(10.0, floor(level));
    let f = fract(level);

    let lines = max(
        max(
            grid_lines(hit.xy, dp, spacing) * level_weight(-f),
            grid_lines(hit.xy, dp, spacing * 10.0) * level_weight(1.0 - f),
        ),
        grid_lines(hit.xy, dp, spacing * 100.0) * level_weight(2.0 - f),
    );

    var color = u.grid.rgb;
    var alpha = lines;

    let axis = dp * 1.2;
    if abs(hit.y) < axis.y {
        color = u.axes[0].rgb;
        alpha = 0.9;
    } else if abs(hit.x) < axis.x {
        color = u.axes[1].rgb;
        alpha = 0.9;
    }

    // Fade out far from the target, and at grazing angles where lines alias.
    let extent = view_height();
    let from_focus = length(hit.xy - u.focus.xy);
    let fade = (1.0 - smoothstep(extent * 1.5, extent * GRID_FADE_HEIGHTS, from_focus))
        * smoothstep(0.02, 0.15, abs(normalize(dir).z));
    // In front of the eye and within the depth range.
    let valid = f32(clip.w > 0.0 && depth >= 0.0 && depth <= 1.0);

    var out: GridOut;
    out.color = output(vec4<f32>(color, alpha * fade * valid));
    out.depth = clamp(depth, 0.0, 1.0);
    return out;
}

// --- Model ---

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct MeshOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_mesh(in: MeshIn) -> MeshOut {
    var out: MeshOut;
    out.position = u.view_proj * vec4<f32>(in.position, 1.0);
    out.world = in.position;
    out.normal = in.normal;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let view = u.backward.xyz;
    let key = normalize(vec3<f32>(0.4, -0.6, 1.0));

    // Bright, low contrast shading.
    let base = u.model.rgb;
    let ambient = mix(vec3<f32>(0.42, 0.42, 0.44), vec3<f32>(0.55, 0.57, 0.60), n.z * 0.5 + 0.5);
    let diffuse = max(dot(n, key), 0.0) * 0.30 + max(dot(n, view), 0.0) * 0.25;
    let spec = pow(max(dot(n, normalize(key + view)), 0.0), 32.0) * 0.15;

    return output(vec4<f32>(base * (ambient + diffuse) + spec, 1.0));
}

// --- Feature edges ---

// Least normalized depth that edges are pulled in by, well clear of
// Depth32Float precision. The depth range is fitted to the scene, so zooming
// into a large one shrinks the pull by view height below that. WebGPU forbids
// pipeline depth bias on lines.
const EDGE_DEPTH_BIAS: f32 = 1e-5;

@vertex
fn vs_edge(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    // Pull towards the camera so edges win the depth test against their faces.
    let offset = u.backward.xyz * view_height() * 0.002;
    let clip = u.view_proj * vec4<f32>(position + offset, 1.0);
    let unpulled = u.view_proj * vec4<f32>(position, 1.0);
    let depth = min(clip.z / clip.w, unpulled.z / unpulled.w - EDGE_DEPTH_BIAS);
    return vec4<f32>(clip.xy, depth * clip.w, clip.w);
}

@fragment
fn fs_edge() -> @location(0) vec4<f32> {
    return output(vec4<f32>(u.edge.rgb, 0.9));
}

// --- Origin marker ---
//
// Short X/Y/Z axes and a dot at the world origin, drawn as screen-space quads
// so they keep a constant on-screen size at any zoom level. Sizes are in
// logical pixels, scaled to physical ones by `u.viewport.z`. Always drawn on
// top, since the origin often coincides with model corners and edges.

struct OriginOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Quad-local coordinates in [-1, 1], used for anti-aliasing and the dot.
    @location(1) local: vec2<f32>,
    @location(2) @interpolate(flat) is_dot: u32,
};

const AXIS_PIXELS: f32 = 70.0;
const AXIS_WIDTH: f32 = 2.5;
const DOT_RADIUS: f32 = 5.0;

fn to_pixels(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy / clip.w * 0.5 * u.viewport.xy;
}

// Depth is forced to the near plane so the marker is never occluded.
fn from_pixels(pixels: vec2<f32>, clip: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(pixels / (0.5 * u.viewport.xy) * clip.w, 0.0, clip.w);
}

@vertex
fn vs_origin(@builtin(vertex_index) index: u32) -> OriginOut {
    // Two triangles per quad, corners in quad-local space.
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let quad = index / 6u;
    let corner = corners[index % 6u];

    let origin = u.view_proj * vec4<f32>(0.0, 0.0, 0.0, 1.0);

    var out: OriginOut;
    out.local = corner;

    if quad == 3u {
        let offset = corner * (DOT_RADIUS + 1.0) * u.viewport.z;
        out.position = from_pixels(to_pixels(origin) + offset, origin);
        out.color = vec4<f32>(1.0);
        out.is_dot = 1u;
        return out;
    }

    var axes = array<vec3<f32>, 3>(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0));

    // Scale the world-space axis so it spans AXIS_PIXELS at the origin.
    let pixel_size = view_height() / u.viewport.y * u.viewport.z;
    let tip = u.view_proj * vec4<f32>(axes[quad] * AXIS_PIXELS * pixel_size, 1.0);

    let a = to_pixels(origin);
    let b = to_pixels(tip);
    let along = b - a;
    let side = normalize(vec2<f32>(-along.y, along.x) + vec2<f32>(1e-6, 0.0)) * AXIS_WIDTH * 0.5 * u.viewport.z;

    let t = corner.x * 0.5 + 0.5;
    let base = select(origin, tip, t > 0.5);
    out.position = from_pixels(mix(a, b, t) + side * corner.y, base);
    out.color = vec4<f32>(u.axes[quad].rgb, 1.0);
    out.is_dot = 0u;
    return out;
}

@fragment
fn fs_origin(in: OriginOut) -> @location(0) vec4<f32> {
    if in.is_dot == 1u {
        // In physical pixels, so the outline scales and anti-aliasing stays
        // one device pixel wide.
        let s = u.viewport.z;
        let radius = DOT_RADIUS * s;
        let r = length(in.local) * (DOT_RADIUS + 1.0) * s;
        let fill = 1.0 - smoothstep(radius - 2.0 * s, radius - s, r);
        let coverage = 1.0 - smoothstep(radius - 0.5, radius + 0.5, r);
        let color = mix(u.origin_outline.rgb, vec3<f32>(1.0), fill);
        return output(vec4<f32>(color, coverage));
    }

    let edge = 1.0 - smoothstep(0.6, 1.0, abs(in.local.y));
    return output(vec4<f32>(in.color.rgb, in.color.a * edge));
}
