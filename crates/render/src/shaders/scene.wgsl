// See `Uniforms` in renderer.rs for what each field holds.
struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    focus: vec4<f32>,
    backward: vec4<f32>,
    viewport: vec4<f32>,
    viewport_origin: vec4<f32>,
    background_top: vec4<f32>,
    background_bottom: vec4<f32>,
    model: vec4<f32>,
    edge: vec4<f32>,
    grid: vec4<f32>,
    axes: array<vec4<f32>, 3>,
    origin_outline: vec4<f32>,
    sketch: vec4<f32>,
    pivot: vec4<f32>,
    pivot_color: vec4<f32>,
    grid_origin: vec4<f32>,
    grid_x: vec4<f32>,
    grid_y: vec4<f32>,
    sketch_origin: vec4<f32>,
    sketch_x: vec4<f32>,
    sketch_y: vec4<f32>,
    hover_face: vec4<f32>,
    hover_outline: vec4<f32>,
    selected: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// See `PartUniforms` in renderer.rs: how opaque the part of the model being
// drawn is, bound at its step's offset for each part's draws.
struct Part {
    alpha: vec4<f32>,
};

@group(1) @binding(0) var<uniform> part: Part;

// See `Slot::tints` in renderer.rs: the linear colour of the part of the
// model being drawn, the model's or its body's own, bound at its entry's
// offset. Binding 3, as the errors' group 0 has 1 and 2.
struct Tint {
    color: vec4<f32>,
};

@group(0) @binding(3) var<uniform> tint: Tint;

// Set by the renderer. True if the target stores output as is, so it must be
// sRGB encoded here, false if the target encodes it.
override ENCODE_SRGB: bool;
// Grid lines fade out within this many view heights of the target. The
// depth range, fitted in scene.rs, covers them.
override GRID_FADE_HEIGHTS: f32;
// How wide sketch lines are, in logical pixels.
override LINE_WIDTH: f32;
// How wide feature edges are, in logical pixels.
override EDGE_WIDTH: f32;
// How wide the edges hidden by the model are, and their dashes' and gaps'
// lengths along them, in logical pixels.
override HIDDEN_EDGE_WIDTH: f32;
// How wide creases are, in logical pixels, and how opaque, of what the
// feature edges are, seen or hidden.
override CREASE_WIDTH: f32;
override CREASE_ALPHA: f32;
override HIDDEN_DASH: f32;
override HIDDEN_GAP: f32;
// How wide the hovered edges are, the rim around them and the hovered
// vertex, how wide the selected edges are, and the radius of a vertex's
// disc within its rim, in logical pixels.
override HOVERED_EDGE_WIDTH: f32;
override HOVER_RIM: f32;
// How opaque a hovered face drawn over what hides it is, of its hover.
override HOVER_THROUGH_ALPHA: f32;
override SELECTED_EDGE_WIDTH: f32;
override SELECTED_RIM: f32;
override VERTEX_RADIUS: f32;
// How wide error geometry's curves are, how far its halo reaches beyond it,
// and the radius of its points' discs, in logical pixels.
override ERROR_EDGE_WIDTH: f32;
override ERROR_HALO: f32;
override ERROR_POINT_RADIUS: f32;
// Set for the pipelines drawing the sketch's layers depth tested: they're
// given their depth, pulled towards the camera, rather than drawn on top.
override SKETCH_DEPTH: bool = false;

// The visible height at the target in world units.
fn view_height() -> f32 {
    return 2.0 * u.up.w;
}

fn perspective() -> bool {
    return u.eye.w > 0.5;
}

// How far in front of the eye a perspective view starts.
fn near() -> f32 {
    return u.focus.w;
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

// The world position `world` in the grid's coordinates: along its x and y
// axes from its origin.
fn in_grid(world: vec3<f32>) -> vec2<f32> {
    let offset = world - u.grid_origin.xyz;
    return vec2<f32>(dot(offset, u.grid_x.xyz), dot(offset, u.grid_y.xyz));
}

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

// Infinite grid on the grid plane (the XY plane, or a sketch's), ray traced
// per pixel. The spacing adapts to zoom: minor lines are kept at least
// MIN_PIXELS logical pixels apart and blend into the next power of ten as
// they get denser.
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
    let normal = cross(u.grid_x.xyz, u.grid_y.xyz);
    let t = dot(u.grid_origin.xyz - origin, normal) / dot(dir, normal);

    let hit = origin + t * dir;
    let clip = u.view_proj * vec4<f32>(hit, 1.0);
    let depth = clip.z / clip.w;
    // Where the ray hits, in the grid's own coordinates.
    let p = in_grid(hit);

    // Compute everything unconditionally so derivatives stay well defined.
    let dp = fwidth(p);
    let pixel = max(length(dp), 1e-6);
    let level = log(pixel * MIN_PIXELS * u.viewport.z) / log(10.0);
    let spacing = pow(10.0, floor(level));
    let f = fract(level);

    let lines = max(
        max(
            grid_lines(p, dp, spacing) * level_weight(-f),
            grid_lines(p, dp, spacing * 10.0) * level_weight(1.0 - f),
        ),
        grid_lines(p, dp, spacing * 100.0) * level_weight(2.0 - f),
    );

    // Fade out far from the target, and at grazing angles where lines alias.
    let extent = view_height();
    let from_focus = length(p - in_grid(u.focus.xyz));
    let fade = (1.0 - smoothstep(extent * 1.5, extent * GRID_FADE_HEIGHTS, from_focus))
        * smoothstep(0.02, 0.15, abs(dot(normalize(dir), normal)));
    // In front of the eye and within the depth range.
    let valid = f32(clip.w > 0.0 && depth >= 0.0 && depth <= 1.0);

    // The axis lines over the grid, not faded: its own, the y axis over
    // the x axis, then the world's Z axis, those shown (see
    // `OriginShown::mask` in renderer.rs).
    let pixel_at = fragment_pixels(in.position);
    let mask = origin_shown();
    let width = vec3<f32>(
        line_width(mask, 0u),
        line_width(mask, 1u),
        line_width(mask, 2u),
    );
    var axes = array<vec2<f32>, 3>(
        axis_line(pixel_at, origin, dir, u.grid_origin.xyz, u.grid_x.xyz, width.x),
        axis_line(pixel_at, origin, dir, u.grid_origin.xyz, u.grid_y.xyz, width.y),
        axis_line(pixel_at, origin, dir, vec3<f32>(0.0), vec3<f32>(0.0, 0.0, 1.0), width.z),
    );
    var axis_colors = array<vec3<f32>, 3>(
        axis_color(u.grid_x.w),
        axis_color(u.grid_y.w),
        u.axes[2].rgb,
    );
    var color = u.grid.rgb;
    // The axis lines alone, see `AXES_ONLY` in renderer.rs.
    var alpha = select(lines * fade * valid, 0.0, (mask & 128u) != 0u);
    var out_depth = clamp(depth, 0.0, 1.0);
    for (var i = 0u; i < 3u; i++) {
        let line = axes[i];
        if (mask & (1u << i)) != 0u && line.x > 0.0 {
            let a = line.x + alpha * (1.0 - line.x);
            color = (axis_colors[i] * line.x + color * alpha * (1.0 - line.x)) / a;
            alpha = a;
            out_depth = line.y;
        }
    }

    var out: GridOut;
    // At its share while crossfading from one plane to another.
    out.color = output(vec4<f32>(color, alpha * part.alpha.x));
    out.depth = out_depth;
    return out;
}

// What's drawn of the origin objects: `OriginShown::mask` in renderer.rs.
// Bits 0 to 2 the grid's x and y axis lines and the world's Z axis's, and
// this; and bits 4 to 6 for the lines hovered, drawn wider.
const ORIGIN_MARKER: u32 = 8u;
const HOVERED_SHIFT: u32 = 4u;

fn origin_shown() -> u32 {
    return u32(u.grid_origin.w);
}

// The colour of a grid's axis line along the world axis `index` names, 0
// to 2, or the grid's for 3: see `Uniforms::grid_x` in renderer.rs.
fn axis_color(index: f32) -> vec3<f32> {
    let i = u32(index);
    if i < 3u {
        return u.axes[i].rgb;
    }
    return u.grid.rgb;
}

// How wide the grid's axis lines are, in logical pixels, and a hovered
// one.
const AXIS_WIDTH: f32 = 1.75;
const HOVERED_AXIS_WIDTH: f32 = 3.5;

// How wide the axis line `i` of `fs_grid` is, by the `mask`.
fn line_width(mask: u32, i: u32) -> f32 {
    return select(AXIS_WIDTH, HOVERED_AXIS_WIDTH, (mask & (1u << (i + HOVERED_SHIFT))) != 0u);
}
// The sines of the angles between an axis and the view direction over
// which its line fades in: gone within about 3 degrees, whole past 11.
const AXIS_FADE: vec2<f32> = vec2<f32>(0.05, 0.2);

// How much of an axis along the unit `axis` shows: none pointing at the
// camera, where its image shrinks to a point, all of it once turned
// AXIS_FADE away. Each axis fades on its own, and a sketch's with the
// grid's it lies on.
fn facing(axis: vec3<f32>) -> f32 {
    return smoothstep(AXIS_FADE.x, AXIS_FADE.y, length(cross(axis, u.backward.xyz)));
}

// The axis line through `through` along `axis` (a unit vector) at the
// pixel `p` (from `fragment_pixels`), whose ray
// runs from `origin` along `dir`: its coverage there and its depth. The
// line is infinite and not faded with distance: the coverage is from the
// pixel's distance to the line's image on screen, the homogeneous line
// through the images of a point on it and of its direction, exact at any
// distance and zoom in either projection, so it's anti-aliased without
// MSAA. It fades out as the axis turns to point at the camera
// (`AXIS_FADE`). The depth is the axis's point nearest the ray's, and the
// line is cut where that point is behind the near plane, so the part of
// the image that's behind the eye isn't drawn.
fn axis_line(
    p: vec2<f32>,
    origin: vec3<f32>,
    dir: vec3<f32>,
    through: vec3<f32>,
    axis: vec3<f32>,
    width: f32,
) -> vec2<f32> {
    // The axis's point nearest the target, so clip coordinates stay small.
    let base = through + axis * dot(u.focus.xyz - through, axis);
    let half_size = 0.5 * u.viewport.xy;
    let ca = u.view_proj * vec4<f32>(base, 1.0);
    let cd = u.view_proj * vec4<f32>(axis, 0.0);
    let pa = vec3<f32>(ca.xy * half_size, ca.w);
    let pd = vec3<f32>(cd.xy * half_size, cd.w);
    let l = cross(pa, pd);
    let span = length(l.xy);
    // Seen end on, the image is a point.
    let seen = span > 1e-6 * length(pa) * length(pd);
    let distance = abs(dot(l, vec3<f32>(p, 1.0))) / max(span, 1e-30);

    // The axis's point nearest the ray, at `base + axis * s`.
    let w0 = origin - base;
    let b = dot(dir, axis);
    let c = dot(dir, dir);
    let denom = c - b * b;
    let s = (c * dot(axis, w0) - b * dot(dir, w0)) / max(denom, 1e-30);
    let q = u.view_proj * vec4<f32>(base + axis * s, 1.0);
    let in_front = denom > 1e-12 * c && q.w > 0.0 && q.z >= 0.0;

    let half_width = 0.5 * width * u.viewport.z;
    let coverage = clamp(half_width + 0.5 - distance, 0.0, 1.0) * f32(seen && in_front)
        * facing(axis);
    return vec2<f32>(coverage, clamp(q.z / max(q.w, 1e-30), 0.0, 1.0));
}

// --- Model ---

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct MeshOut {
    // Invariant, so a face drawn again over itself by another program is
    // at exactly the depth it was.
    @builtin(position) @invariant position: vec4<f32>,
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

// How the faces are lit, `Shading` in renderer.rs, kept in the viewport
// origin's unused z.
const SHADING_FLAT: f32 = 1.0;
const SHADING_METAL: f32 = 2.0;
const SHADING_FLAT_METAL: f32 = 3.0;

// About the middle of the light the regular shading gives a face, which
// `Colors::contrast` spreads it from.
const LIGHT_MIDDLE: f32 = 0.8;

// The face at `in` of colour `base` lit as `u.viewport_origin.z` says: by
// default bright, low contrast shading (as `Colors::contrast` spreads it); flat, each triangle lit by its own
// plane's normal, so the tessellation shows; as polished metal (see
// `metal`); or as metal lit flat. A back face, drawn only for a part less
// than opaque, is lit as seen from inside.
fn shaded(in: MeshOut, front: bool, base: vec3<f32>) -> vec3<f32> {
    // The triangle's own normal, from how the position changes across the
    // screen, turned to the side the interpolated normal is on. Worked out
    // whatever the shading, as derivatives need uniform control flow.
    let across = cross(dpdx(in.world), dpdy(in.world));
    let interpolated = normalize(in.normal);
    var normal = interpolated;
    let shading = u.viewport_origin.z;
    let flat = shading == SHADING_FLAT || shading == SHADING_FLAT_METAL;
    if flat && dot(across, across) > 0.0 {
        let plane = normalize(across);
        normal = select(-plane, plane, dot(plane, interpolated) >= 0.0);
    }
    let n = select(-1.0, 1.0, front) * normal;
    let view = u.backward.xyz;
    let key = normalize(vec3<f32>(0.4, -0.6, 1.0));

    if shading == SHADING_METAL || shading == SHADING_FLAT_METAL {
        return metal(n, key, base);
    }
    let ambient = mix(vec3<f32>(0.42, 0.42, 0.44), vec3<f32>(0.55, 0.57, 0.60), n.z * 0.5 + 0.5);
    let diffuse = max(dot(n, key), 0.0) * 0.30 + max(dot(n, view), 0.0) * 0.25;
    // `Colors::contrast`: the light spread from its middle, LIGHT_MIDDLE,
    // and the highlight with it.
    let contrast = u.viewport_origin.w;
    let light = max(vec3<f32>(LIGHT_MIDDLE) + (ambient + diffuse - LIGHT_MIDDLE) * contrast, vec3<f32>(0.0));
    let spec = pow(max(dot(n, normalize(key + view)), 0.0), 32.0) * 0.15 * contrast;
    return base * light + spec;
}

// Normal `n` as polished metal of colour `base`, reflecting a studio fixed
// to the view, as a matcap is, so it reads the same from every side: a
// floor below, a bright horizon and a softer sky above, two tall softboxes
// either side and the `key` light's glint. Towards grazing angles it
// reflects more and its colour less (Schlick's Fresnel).
fn metal(n: vec3<f32>, key: vec3<f32>, base: vec3<f32>) -> vec3<f32> {
    // The normal in view space, x right, y up, z towards the eye, and the
    // ray from the eye reflected off it.
    let v = vec3<f32>(dot(n, u.right.xyz), dot(n, u.up.xyz), dot(n, u.backward.xyz));
    let r = vec3<f32>(2.0 * v.z * v.x, 2.0 * v.z * v.y, 2.0 * v.z * v.z - 1.0);
    var studio = mix(0.25, 1.0, smoothstep(-0.8, -0.35, r.y));
    studio = mix(studio, 0.6, smoothstep(-0.2, 0.6, r.y));
    let boxes = 1.0 - smoothstep(0.04, 0.12, abs(r.x + 0.6))
        + 0.7 * (1.0 - smoothstep(0.03, 0.1, abs(r.x - 0.45)));
    studio = mix(studio, 1.0, boxes * smoothstep(-0.9, -0.5, r.y));
    let glint = pow(max(dot(reflect(-u.backward.xyz, n), key), 0.0), 80.0) * 0.6;
    let fresnel = pow(1.0 - clamp(v.z, 0.0, 1.0), 5.0);
    let tint = mix(base, vec3<f32>(1.0), fresnel);
    return tint * (0.1 + 0.9 * studio) + glint;
}

@fragment
fn fs_mesh(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // In its part's colour; less than opaque when faded, or as its part
    // is, blended over what's behind it.
    return output(vec4<f32>(shaded(in, front, tint.color.rgb), u.model.a * part.alpha.x));
}

// The hovered face, drawn again over itself (depth tested Equal) in the
// hover's colour, lit the same, as opaque as its part.
@fragment
fn fs_hover_face(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return output(vec4<f32>(shaded(in, front, u.hover_face.rgb), part.alpha.x));
}

// A hovered face drawn again over everything, what hides it included,
// HOVER_THROUGH_ALPHA as opaque.
@fragment
fn fs_hover_face_through(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let alpha = HOVER_THROUGH_ALPHA * part.alpha.x;
    return output(vec4<f32>(shaded(in, front, u.hover_face.rgb), alpha));
}

// A selected face, likewise, tinted `u.hover_face.w` of the way towards
// the selection's colour, over the hover if it's hovered too.
@fragment
fn fs_selected_face(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return output(vec4<f32>(shaded(in, front, u.selected.rgb), u.hover_face.w * part.alpha.x));
}

// How the parts of a selected or hovered face something hides are drawn over it: a
// wash of the colour the face is shown in, with diagonal stripes across it on the
// screen SELECTED_STRIPE logical pixels apart, half of that wide, shaded as
// the face is shown, within an edge in the stripes' colour where it ends
// (`fs_pattern_edge`). How opaque the wash is is `u.backward.w`, the
// stripes and the edge `u.viewport.w`, and how wide the edge is, in
// logical pixels, `u.hover_outline.w` (`PatternStyle` in renderer.rs).
const SELECTED_STRIPE: f32 = 8.0;

// A selected face where something hides it (depth tested Greater), over
// everything: the wash with diagonal stripes, anti-aliased, in the colour
// it's shown in, its part's tinted towards the selection's.
@fragment
fn fs_selected_face_hidden(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return hidden_face(in, front, tint.color.rgb, u.hover_face.w);
}

// A selected face that's hovered too, likewise from the hover's colour.
@fragment
fn fs_hovered_selected_face_hidden(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return hidden_face(in, front, u.hover_face.rgb, u.hover_face.w);
}

// A hovered face where something hides it, likewise in the hover's colour.
@fragment
fn fs_hovered_face_hidden(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return hidden_face(in, front, u.hover_face.rgb, 0.0);
}

// `base` shaded, tinted `selected` of the way towards the selection's
// colour as the target blends the selected face over it: in what's
// stored, encoded or not; at the stripes' alpha in them, the wash's
// between.
fn hidden_face(in: MeshOut, front: bool, base: vec3<f32>, selected: f32) -> vec4<f32> {
    // From where the world's origin shows, so panning carries the stripes
    // with the model; from the middle while it's behind the eye.
    let origin = u.view_proj * vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let anchor = select(vec2<f32>(0.0), to_pixels(origin), origin.w > 0.0);
    let p = fragment_pixels(in.position) - anchor;
    let period = SELECTED_STRIPE * u.viewport.z;
    // Distance from the middle of the nearest stripe, in pixels across it.
    let across = abs(wrapped((p.x + p.y) * inverseSqrt(2.0), period) - 0.5 * period);
    let stripe = clamp(0.25 * period + 0.5 - across, 0.0, 1.0);
    let alpha = mix(u.backward.w, u.viewport.w, stripe) * part.alpha.x;
    let under = output(vec4<f32>(shaded(in, front, base), 1.0)).rgb;
    let over = output(vec4<f32>(shaded(in, front, u.selected.rgb), 1.0)).rgb;
    return vec4<f32>(mix(under, over, selected), alpha);
}

// The pattern again, as a mask: where it's drawn, the colour of its
// stripes, into a target of its own, drawn as the pattern is; then its edge
// drawn over the frame from that (`fs_pattern_edge`): the edge of what's
// hidden, not of the face. The hover's is told from the selection's by its
// alpha, PATTERN_HOVERED, the selection's 1, with the colour scaled by it
// so the samples' average over transparent divides back to it. Where the
// faces show, the mask holds PATTERN_SHOWN (`fs_face_shown`), so the edge
// follows the faces' ends, not where something in front cuts them off.
const PATTERN_HOVERED: f32 = 0.5;
const PATTERN_SHOWN: f32 = 0.2;

@fragment
fn fs_face_shown() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, PATTERN_SHOWN);
}

fn pattern(color: vec3<f32>, kind: f32) -> vec4<f32> {
    return vec4<f32>(color * kind, kind);
}

@fragment
fn fs_selected_face_pattern(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return pattern(hidden_face(in, front, tint.color.rgb, u.hover_face.w).rgb, 1.0);
}

@fragment
fn fs_hovered_selected_face_pattern(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return pattern(hidden_face(in, front, u.hover_face.rgb, u.hover_face.w).rgb, 1.0);
}

@fragment
fn fs_hovered_face_pattern(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return pattern(hidden_face(in, front, u.hover_face.rgb, 0.0).rgb, PATTERN_HOVERED);
}

// Which pattern a texel of the mask is of: -1 none, 0 a face showing, 1
// the hover's, 2 the selection's.
fn pattern_kind(alpha: f32) -> i32 {
    if alpha < 0.5 * PATTERN_SHOWN {
        return -1;
    }
    return i32(round(alpha / PATTERN_HOVERED));
}

// The pattern's edge over the frame, once: within the mask (`coverage`),
// where it's no further than the edge's width from outside both it and
// the faces showing, in its colour (none where the hover's meets the
// selection's, or where the face shows on) at the stripes' alpha. What's read is as
// stored, encoded or not, so it's written as it is.
@fragment
fn fs_pattern_edge(in: FullscreenOut) -> @location(0) vec4<f32> {
    let at = vec2<i32>(in.position.xy);
    let here = textureLoad(coverage, at, 0);
    let kind = pattern_kind(here.a);
    if kind <= 0 {
        discard;
    }
    let size = vec2<i32>(textureDimensions(coverage));
    let radius = u.hover_outline.w * u.viewport.z;
    let reach = i32(ceil(radius));
    var edge = false;
    for (var y = -reach; y <= reach && !edge; y++) {
        for (var x = -reach; x <= reach; x++) {
            let off = vec2<f32>(f32(x), f32(y));
            if dot(off, off) > radius * radius {
                continue;
            }
            let p = at + vec2<i32>(x, y);
            let inside = all(p >= vec2<i32>(0)) && all(p < size);
            if !inside || pattern_kind(textureLoad(coverage, p, 0).a) < 0 {
                edge = true;
                break;
            }
        }
    }
    if !edge {
        discard;
    }
    // As much of the pixel as the pattern covers.
    let full = select(1.0, PATTERN_HOVERED, kind == 1);
    let covered = min(here.a / full, 1.0);
    return vec4<f32>(here.rgb / here.a, u.viewport.w * covered);
}

// The second colour (the measure tool's B), kept in the sketch plane's
// unused w's: the uniforms have no room for another vector.
fn second_color() -> vec3<f32> {
    return vec3<f32>(u.sketch_origin.w, u.sketch_x.w, u.sketch_y.w);
}

// A face in the second colour, tinted as a selected face is.
@fragment
fn fs_second_face(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return output(vec4<f32>(shaded(in, front, second_color()), u.hover_face.w * part.alpha.x));
}

// --- Feature edges ---

// Least normalized depth that edges are pulled in by, well clear of the
// depth buffer's precision: 24 bits where the device has no
// Depth32FloatStencil8, which iced's devices don't ask for. The depth range
// is fitted to the scene, so zooming into a large one shrinks the pull by
// view height below that.
const EDGE_DEPTH_BIAS: f32 = 1e-5;

// `position` in clip space, its depth pulled towards the camera so that
// edges and lines win the depth test against the faces they lie on (a
// pipeline's depth bias scales with the faces' slope instead). Only the
// depth changes: in perspective the pulled point would show elsewhere.
// Never before the near plane, which a pull from just behind it would clip.
fn pulled(position: vec3<f32>) -> vec4<f32> {
    let offset = u.backward.xyz * view_height() * 0.002;
    let clip = u.view_proj * vec4<f32>(position + offset, 1.0);
    let unpulled = u.view_proj * vec4<f32>(position, 1.0);
    let depth = max(min(clip.z / clip.w, unpulled.z / unpulled.w - EDGE_DEPTH_BIAS), 0.0);
    return vec4<f32>(unpulled.xy, depth * unpulled.w, unpulled.w);
}

// --- Lines ---
//
// Polylines, drawn a segment per instance as a quad around it, a pixel
// wider than the line: the fragment shader turns the distance from the
// segment into coverage, so lines are anti-aliased and their ends and joins
// round. A segment knows its neighbours along the polyline, and where they
// overlap at a join a pixel is drawn only by the one nearest it, so it
// isn't blended twice. Dashes run on along the polyline, faded at their
// ends. Everything is in physical pixels from the viewport's centre, y up
// (see `to_pixels`).
//
// Feature edges (`vs_edge`) are depth tested and pulled, so an edge shows
// on the faces it bounds while bodies in front of it hide it, and drawn
// again dashed where they hide it (`vs_hidden_edge`); they come from a
// stream of points, a segment per instance that sees the points either
// side of it. Finished sketches' lines (`vs_line`) are depth tested
// and pulled the same way; they come a segment at a time, without
// neighbours. The sketch being edited (`vs_sketch_line`) is drawn over
// everything.

// Flags of a segment of the sketch being edited, as in sketch.rs: whether
// it has a neighbour before and after it, and whether it's in logical
// pixels from the viewport's top left, or in the world (x and y with z
// apart), rather than sketch coordinates.
const HAS_PREV: u32 = 1u;
const HAS_NEXT: u32 = 2u;
const SCREEN: u32 = 4u;
const WORLD: u32 = 8u;
const FADES: u32 = 16u;

struct LineOut {
    // Invariant, so the hidden edges' pass, from another entry point, puts
    // a pixel at exactly the depth the visible edges' did.
    @builtin(position) @invariant position: vec4<f32>,
    // The segment's start and end.
    @location(0) @interpolate(flat) ends: vec4<f32>,
    // The segments before and after it, where `flags` say there are.
    @location(1) @interpolate(flat) prev: vec4<f32>,
    @location(2) @interpolate(flat) next: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    // x: half the width; y, z: a dash's and a gap's lengths, 0 for a solid
    // line; w: half the width of the middle left out of a hollow line, 0
    // for a full one.
    @location(4) @interpolate(flat) style: vec4<f32>,
    // How far along the polyline the segment's start and end are.
    @location(5) @interpolate(flat) along: vec2<f32>,
    // HAS_PREV and HAS_NEXT.
    @location(6) @interpolate(flat) flags: u32,
    // For `fs_highlight_line`: see `highlight_slope`.
    @location(7) @interpolate(flat) slope: f32,
};

// A segment as it's drawn: its ends, and where they are along the segment
// it was cut from, as fractions of the way from its start. Nothing shows if
// `t.x > t.y`.
struct Shown {
    ends: vec4<f32>,
    t: vec2<f32>,
};

// The fractions of the way along the segment from clip positions `a` to `b`
// that are in front of the near plane of a perspective view: all of it in
// an orthographic one. Clip space is linear in world space, so the cut is
// where their `w` mix to the near distance.
fn in_front(a: vec4<f32>, b: vec4<f32>) -> vec2<f32> {
    if !perspective() {
        return vec2<f32>(0.0, 1.0);
    }
    let da = a.w - near();
    let db = b.w - near();
    if da < 0.0 && db < 0.0 {
        return vec2<f32>(1.0, 0.0);
    }
    if da < 0.0 {
        return vec2<f32>(da / (da - db), 1.0);
    }
    if db < 0.0 {
        return vec2<f32>(0.0, da / (da - db));
    }
    return vec2<f32>(0.0, 1.0);
}

// Cuts the fractions `t` of the way from `a` to `b`, `d` apart, on one axis
// to the slab from `-limit` to `limit` (Liang-Barsky).
fn slab(t: vec2<f32>, a: f32, d: f32, limit: f32) -> vec2<f32> {
    if d == 0.0 {
        return select(vec2<f32>(1.0, 0.0), t, abs(a) <= limit);
    }
    let t0 = (-limit - a) / d;
    let t1 = (limit - a) / d;
    return vec2<f32>(max(t.x, min(t0, t1)), min(t.y, max(t0, t1)));
}

// The part of the segment between clip positions `a` and `b` that shows:
// in front of the near plane if `cut` (the screen has nothing behind it),
// and within `margin` pixels of the viewport, so pixel coordinates stay
// small enough to be exact where it's drawn. Neighbouring segments cut each
// other's ends the same way, so they meet where the same pixels are
// compared.
//
// Worked out from the end nearer the middle of the view: the cuts mix the
// ends by fractions of the way from the start, which are only as exact as
// the start is near where they fall. From an end far off, as a sketch's
// axes reach, the part that shows was pixels off.
fn shown(a: vec4<f32>, b: vec4<f32>, cut: bool, margin: f32) -> Shown {
    if !(off_centre(b) < off_centre(a)) {
        return shown_from(a, b, cut, margin);
    }
    var out = shown_from(b, a, cut, margin);
    out.ends = out.ends.zwxy;
    out.t = 1.0 - out.t.yx;
    return out;
}

// How far the clip position `c` is from the middle of the view, for
// `shown` to start from the nearer end: in the view's half sizes, or, at
// or behind the eye, as far as can be.
fn off_centre(c: vec4<f32>) -> f32 {
    return select(1e30, max(abs(c.x), abs(c.y)) / c.w, c.w > 0.0);
}

// `shown`, worked out from `a`.
fn shown_from(a: vec4<f32>, b: vec4<f32>, cut: bool, margin: f32) -> Shown {
    var out: Shown;
    var front = vec2<f32>(0.0, 1.0);
    if cut {
        front = in_front(a, b);
    }
    out.t = front;
    if front.x > front.y {
        return out;
    }
    let pa = to_pixels(mix(a, b, front.x));
    let pb = to_pixels(mix(a, b, front.y));
    let limit = 0.5 * u.viewport.xy + margin;
    let d = pb - pa;
    let seen = slab(slab(vec2<f32>(0.0, 1.0), pa.x, d.x, limit.x), pa.y, d.y, limit.y);
    out.ends = vec4<f32>(mix(pa, pb, seen.x), mix(pa, pb, seen.y));
    out.t = mix(vec2<f32>(front.x), vec2<f32>(front.y), seen);
    return out;
}

// Vertex `index` of the quad around `own`, its ends at `depth` (normalized,
// where its start and end show), `half` pixels either side of it, between
// `prev` and `next` where `flags` say there are. Coordinates of the quad
// are divided through (w = 1), so fragments see pixel positions that
// neighbouring quads agree on.
fn line_vertex(
    index: u32,
    own: Shown,
    prev: Shown,
    next: Shown,
    flags: u32,
    depth: vec2<f32>,
    half: f32,
    color: vec4<f32>,
    dash: vec2<f32>,
    along: vec2<f32>,
) -> LineOut {
    var out: LineOut;
    if own.t.x > own.t.y {
        // Nothing of it shows.
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    let pa = own.ends.xy;
    let pb = own.ends.zw;
    let length = distance(pa, pb);
    // A segment seen end on is a square around its end.
    let dir = select(vec2<f32>(1.0, 0.0), (pb - pa) / length, length > 1e-6);
    let side = vec2<f32>(-dir.y, dir.x);
    // A pixel more for anti-aliasing.
    let reach = half + 1.0;

    // Two triangles, corners as (which end, which side).
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(0.0, -1.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let corner = corners[index];
    let at_end = corner.x > 0.5;
    let pixels = select(pa - dir * reach, pb + dir * reach, at_end) + side * reach * corner.y;
    let z = select(depth.x, depth.y, at_end);
    out.position = vec4<f32>(pixels / (0.5 * u.viewport.xy), z, 1.0);
    out.ends = own.ends;
    out.prev = prev.ends;
    out.next = next.ends;
    out.color = color;
    out.style = vec4<f32>(half, dash, 0.0);
    out.along = mix(vec2<f32>(along.x), vec2<f32>(along.y), own.t);
    // A neighbour that doesn't show is none.
    out.flags = flags;
    if prev.t.x > prev.t.y {
        out.flags &= ~HAS_PREV;
    }
    if next.t.x > next.t.y {
        out.flags &= ~HAS_NEXT;
    }
    return out;
}

// `x` modulo `period`, from 0 up to `period`.
fn wrapped(x: f32, period: f32) -> f32 {
    return x - floor(x / period) * period;
}

// Nothing: for a neighbour there isn't.
fn none() -> Shown {
    var out: Shown;
    out.t = vec2<f32>(1.0, 0.0);
    return out;
}

// A segment of the world as it's drawn pulled (see `pulled`): what shows
// of it, its `t` fractions of the whole segment, and the normalized depth
// where its ends show.
struct Pulled {
    shown: Shown,
    depth: vec2<f32>,
};

// The segment from `start` to `end` in the world, pulled, as it's drawn
// within `margin` pixels of the viewport. It's cut to the near plane, where
// clip space z is 0, before pulling, which needs the depth of a point in
// front of the eye. Clip space is linear in world space, so the cut is
// where the ends' z mix to 0. Segments that meet compute their neighbours
// with this too, so they agree on where they are.
fn pulled_segment(start: vec3<f32>, end: vec3<f32>, margin: f32) -> Pulled {
    var out: Pulled;
    out.shown = none();
    let za = (u.view_proj * vec4<f32>(start, 1.0)).z;
    let zb = (u.view_proj * vec4<f32>(end, 1.0)).z;
    if za < 0.0 && zb < 0.0 {
        return out;
    }
    var a = start;
    var b = end;
    var cut = vec2<f32>(0.0, 1.0);
    if za < 0.0 {
        cut.x = za / (za - zb);
        a = mix(start, end, cut.x);
    } else if zb < 0.0 {
        let back = zb / (zb - za);
        cut.y = 1.0 - back;
        b = mix(end, start, back);
    }

    let ca = pulled(a);
    let cb = pulled(b);
    // Already cut, before pulling.
    let own = shown(ca, cb, false, margin);
    if own.t.x > own.t.y {
        return out;
    }
    out.depth = mix(vec2<f32>(ca.z / ca.w), vec2<f32>(cb.z / cb.w), own.t);
    out.shown = own;
    out.shown.t = mix(vec2<f32>(cut.x), vec2<f32>(cut.y), own.t);
    return out;
}

@vertex
fn vs_line(
    @builtin(vertex_index) index: u32,
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
) -> LineOut {
    let half = LINE_WIDTH * 0.5 * u.viewport.z;
    let own = pulled_segment(start, end, half + 2.0);
    return line_vertex(index, own.shown, none(), none(), 0u, own.depth, half,
        vec4<f32>(u.sketch.rgb, 1.0), vec2<f32>(0.0), vec2<f32>(0.0));
}

// An edge of the mesh's triangles, in a tessellation wireframe: faint and
// thin, as creases are, as opaque as the model and its part.
@vertex
fn vs_triangle_edge(
    @builtin(vertex_index) index: u32,
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
) -> LineOut {
    let half = CREASE_WIDTH * 0.5 * u.viewport.z;
    let own = pulled_segment(start, end, half + 2.0);
    let color = vec4<f32>(u.edge.rgb, u.model.a * part.alpha.x * CREASE_ALPHA);
    return line_vertex(index, own.shown, none(), none(), 0u, own.depth, half, color,
        vec2<f32>(0.0), vec2<f32>(0.0));
}

// Set in an edge point's edge where it's only a neighbour of its edge's
// segments, as in renderer.rs: where a closed polyline joins itself.
const NEIGHBOUR_ONLY: u32 = 0x80000000u;
// Set in a crease's points' edge, as in renderer.rs: a feature edge inside
// one face.
const CREASE: u32 = 0x40000000u;

// How opaque an edge of `in` is drawn, of the feature edges: CREASE_ALPHA
// for a crease.
fn crease_alpha(in: EdgeIn) -> f32 {
    return select(1.0, CREASE_ALPHA, (in.start_edge & CREASE) != 0u);
}

// A feature edge's segment's instance: see `edge_segment`.
struct EdgeIn {
    @builtin(vertex_index) index: u32,
    @location(0) prev: vec3<f32>,
    @location(1) prev_edge: u32,
    @location(2) start: vec3<f32>,
    // How far along its edge the start is, in world units.
    @location(3) start_along: f32,
    @location(4) start_edge: u32,
    @location(5) end: vec3<f32>,
    @location(6) end_edge: u32,
    @location(7) next: vec3<f32>,
    @location(8) next_edge: u32,
};

// A segment of a feature edge, from the stream of the edges' points (see
// `EdgePoint` in renderer.rs): drawn if its start and end are of the same
// edge, and joined to the points either side that are of it too, marked
// NEIGHBOUR_ONLY or not. The quad reaches `quad` pixels either side, the
// same for the visible and hidden passes, so they split the same pixels
// between them; `half`, `color`, `dash` and `along` are as in
// `line_vertex`.
fn edge_segment(
    in: EdgeIn,
    quad: f32,
    half: f32,
    color: vec4<f32>,
    dash: vec2<f32>,
    along: vec2<f32>,
) -> LineOut {
    if in.start_edge != in.end_edge || (in.start_edge & NEIGHBOUR_ONLY) != 0u {
        // Between two edges, or from or to a point only a neighbour.
        return line_vertex(in.index, none(), none(), none(), 0u, vec2<f32>(0.0), 0.0,
            vec4<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
    }
    let margin = quad + 2.0;
    var flags = 0u;
    var before = none();
    if (in.prev_edge & ~NEIGHBOUR_ONLY) == in.start_edge {
        before = pulled_segment(in.prev, in.start, margin).shown;
        flags |= HAS_PREV;
    }
    var after = none();
    if (in.next_edge & ~NEIGHBOUR_ONLY) == in.end_edge {
        after = pulled_segment(in.end, in.next, margin).shown;
        flags |= HAS_NEXT;
    }
    let own = pulled_segment(in.start, in.end, margin);
    var out = line_vertex(in.index, own.shown, before, after, flags, own.depth, quad, color,
        dash, along);
    out.style.x = half;
    return out;
}

// The feature edges where the model doesn't hide them, as opaque as the
// model (faded or not) and their part; creases CREASE_WIDTH wide and fainter,
// within the same quads.
@vertex
fn vs_edge(in: EdgeIn) -> LineOut {
    let color = vec4<f32>(u.edge.rgb, u.model.a * part.alpha.x * crease_alpha(in));
    let s = u.viewport.z;
    let width = select(EDGE_WIDTH, CREASE_WIDTH, (in.start_edge & CREASE) != 0u);
    var out = edge_segment(in, EDGE_WIDTH * 0.5 * s, width * 0.5 * s, color, vec2<f32>(0.0),
        vec2<f32>(0.0));
    out.slope = highlight_slope(mix(in.start, in.end, 0.5));
    return out;
}

// The feature edges where the model hides them (depth tested Greater, so
// exactly the pixels `vs_edge` didn't draw): HIDDEN_EDGE_WIDTH wide, dashed
// at the target's scale, at `u.edge.a` times their part's alpha (and
// CREASE_ALPHA for creases). The phase is wrapped a segment at a time, at
// its start and again where it's cut, so it keeps its precision zoomed far
// into a long edge.
@vertex
fn vs_hidden_edge(in: EdgeIn) -> LineOut {
    let s = u.viewport.z;
    let dash = vec2<f32>(HIDDEN_DASH, HIDDEN_GAP) * s;
    let period = dash.x + dash.y;
    // Physical pixels per world unit at the target.
    let scale = u.viewport.y / view_height();
    let phase = wrapped(in.start_along * scale, period);
    let length = distance(in.start, in.end) * scale;
    let color = vec4<f32>(u.edge.rgb, u.edge.a * part.alpha.x * crease_alpha(in));
    var out = edge_segment(in, EDGE_WIDTH * 0.5 * s, HIDDEN_EDGE_WIDTH * 0.5 * s, color, dash,
        vec2<f32>(phase, phase + length));
    out.along -= vec2<f32>(floor(out.along.x / period) * period);
    out.slope = highlight_slope(mix(in.start, in.end, 0.5));
    return out;
}

// Normalized depth a highlight's or feature edge's pixel is pulled in by
// per physical pixel from its middle, at `at`, so a face that falls away
// steeply from the edge or vertex it borders doesn't hide its outer pixels
// (nor, at a bend in an edge, the pixels past a segment's end, which left
// gaps at its joins): HIGHLIGHT_SLOPE
// pixels' worth of the world towards the eye, so faces as steep as about
// 18 degrees from the line of sight. The middle is pulled only as the
// edges are, so a highlight's hidden as they are.
const HIGHLIGHT_SLOPE: f32 = 3.0;

fn highlight_slope(at: vec3<f32>) -> f32 {
    let pixel = view_height() / u.viewport.y;
    let a = u.view_proj * vec4<f32>(at, 1.0);
    let b = u.view_proj * vec4<f32>(at + u.backward.xyz * pixel * HIGHLIGHT_SLOPE, 1.0);
    if a.w <= 0.0 || b.w <= 0.0 {
        return 0.0;
    }
    return max(a.z / a.w - b.z / b.w, 0.0);
}

// A highlight's line from `edge_segment`, its depth pulled in by its
// distance from the edge (`highlight_slope`).
fn highlight_segment(in: EdgeIn, half: f32, color: vec4<f32>) -> LineOut {
    var out = edge_segment(in, half, half, color, vec2<f32>(0.0), vec2<f32>(0.0));
    out.slope = highlight_slope(mix(in.start, in.end, 0.5));
    return out;
}

struct HighlightOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_highlight_line(in: LineOut) -> HighlightOut {
    return highlight_line(in);
}

fn highlight_line(in: LineOut) -> HighlightOut {
    let own = segment_distance(fragment_pixels(in.position), in.ends.xy, in.ends.zw);
    var out: HighlightOut;
    out.color = line_color(in, own);
    out.depth = max(in.position.z - in.slope * own.x, 0.0);
    return out;
}

@fragment
fn fs_highlight_point(in: PointOut) -> HighlightOut {
    return highlight_point(in);
}

fn highlight_point(in: PointOut) -> HighlightOut {
    var out: HighlightOut;
    out.color = point_color(in);
    let r = distance(fragment_pixels(in.position), in.center);
    out.depth = max(in.position.z - in.slope * r, 0.0);
    return out;
}

// The rim of the hovered edges' outline: HOVER_RIM wide either side of
// their HOVERED_EDGE_WIDTH, in `u.hover_outline`, depth tested and pulled
// like the edges. It's hollow (`style.w`), leaving the edge to
// `vs_hovered_edge`. Outlined edges that meet come as one polyline
// (`Highlights::build` in highlight.rs), so neither's rim covers the
// other.
@vertex
fn vs_outline(in: EdgeIn) -> LineOut {
    return outline(in);
}

fn outline(in: EdgeIn) -> LineOut {
    let core = HOVERED_EDGE_WIDTH * 0.5 * u.viewport.z;
    let half = core + HOVER_RIM * u.viewport.z;
    var out = highlight_segment(in, half, vec4<f32>(u.hover_outline.rgb, 1.0));
    out.style.w = core;
    return out;
}

// How opaque the rim around the selected edges and vertices is, in the
// hover's rim colour.
const SELECTED_RIM_ALPHA: f32 = 0.5;

// The rim around the selected edges: SELECTED_RIM wide either side of
// their SELECTED_EDGE_WIDTH, `u.hover_outline` at SELECTED_RIM_ALPHA, for
// contrast with what's behind them; hollow, as the hover's is.
@vertex
fn vs_selected_outline(in: EdgeIn) -> LineOut {
    let core = SELECTED_EDGE_WIDTH * 0.5 * u.viewport.z;
    let half = core + SELECTED_RIM * u.viewport.z;
    var out = highlight_segment(in, half, vec4<f32>(u.hover_outline.rgb, SELECTED_RIM_ALPHA));
    out.style.w = core;
    return out;
}

// The rim around the edges in the second colour, as the selected edges'.
@vertex
fn vs_second_outline(in: EdgeIn) -> LineOut {
    let core = SELECTED_EDGE_WIDTH * 0.5 * u.viewport.z;
    let half = core + SELECTED_RIM * u.viewport.z;
    var out = highlight_segment(in, half, vec4<f32>(u.hover_outline.rgb, SELECTED_RIM_ALPHA));
    out.style.w = core;
    return out;
}

// The hovered edges, HOVERED_EDGE_WIDTH wide in the edges' colour, within
// their rim.
@vertex
fn vs_hovered_edge(in: EdgeIn) -> LineOut {
    return hovered_edge(in);
}

fn hovered_edge(in: EdgeIn) -> LineOut {
    let half = HOVERED_EDGE_WIDTH * 0.5 * u.viewport.z;
    return highlight_segment(in, half, vec4<f32>(u.edge.rgb, 1.0));
}

// The same two drawn again over everything: entry points of their own,
// which wgpu's GL backend keys programs by.
@vertex
fn vs_outline_through(in: EdgeIn) -> LineOut {
    return outline(in);
}

@vertex
fn vs_hovered_edge_through(in: EdgeIn) -> LineOut {
    return hovered_edge(in);
}

// The selected edges' and vertices' colour: the selection's, shaded by
// `u.selected.w` towards black or white, so it stands out on a selected
// face.
fn selected_edge() -> vec4<f32> {
    let shade = u.selected.w;
    let towards = select(vec3<f32>(0.0), vec3<f32>(1.0), shade > 0.0);
    return vec4<f32>(mix(u.selected.rgb, towards, abs(shade)), 1.0);
}

// The selected edges, SELECTED_EDGE_WIDTH wide in the selection's colour,
// over the edges and their outline, depth tested and pulled like them.
@vertex
fn vs_selected_edge(in: EdgeIn) -> LineOut {
    let half = SELECTED_EDGE_WIDTH * 0.5 * u.viewport.z;
    return highlight_segment(in, half, selected_edge());
}

// The selected edges where something hides them (depth tested Greater),
// over everything: dashed as the hidden edges are, as wide as the
// selected, within their rim, dashed too.
@vertex
fn vs_selected_outline_hidden(in: EdgeIn) -> LineOut {
    let core = SELECTED_EDGE_WIDTH * 0.5;
    let color = vec4<f32>(u.hover_outline.rgb, SELECTED_RIM_ALPHA);
    return dashed_highlight(in, core + SELECTED_RIM, core, color);
}

@vertex
fn vs_selected_edge_hidden(in: EdgeIn) -> LineOut {
    return dashed_highlight(in, SELECTED_EDGE_WIDTH * 0.5, 0.0, selected_edge());
}

// The hovered edges where something hides them, likewise.
@vertex
fn vs_outline_hidden(in: EdgeIn) -> LineOut {
    let core = HOVERED_EDGE_WIDTH * 0.5;
    return dashed_highlight(in, core + HOVER_RIM, core, vec4<f32>(u.hover_outline.rgb, 1.0));
}

@vertex
fn vs_hovered_edge_hidden(in: EdgeIn) -> LineOut {
    return dashed_highlight(in, HOVERED_EDGE_WIDTH * 0.5, 0.0, vec4<f32>(u.edge.rgb, 1.0));
}

// An edge `half` logical pixels wide either side in `color`, dashed as
// the hidden edges are, hollow `hollow` either side if that's above 0.
fn dashed_highlight(in: EdgeIn, half: f32, hollow: f32, color: vec4<f32>) -> LineOut {
    let s = u.viewport.z;
    let dash = vec2<f32>(HIDDEN_DASH, HIDDEN_GAP) * s;
    let period = dash.x + dash.y;
    let scale = u.viewport.y / view_height();
    let phase = wrapped(in.start_along * scale, period);
    let length = distance(in.start, in.end) * scale;
    var out = edge_segment(in, half * s, half * s, color, dash, vec2<f32>(phase, phase + length));
    out.along -= vec2<f32>(floor(out.along.x / period) * period);
    out.slope = highlight_slope(mix(in.start, in.end, 0.5));
    out.style.w = hollow * s;
    return out;
}

// The edges in the second colour, as wide as the selected edges, in it
// unshaded: the selection's shade, far towards white in the dark theme,
// would wash it out, and it stands apart from the selection's tint as
// it is.
@vertex
fn vs_second_edge(in: EdgeIn) -> LineOut {
    let half = SELECTED_EDGE_WIDTH * 0.5 * u.viewport.z;
    return highlight_segment(in, half, vec4<f32>(second_color(), 1.0));
}

// Where the sketch point `at` is in the world.
fn on_plane(at: vec2<f32>) -> vec3<f32> {
    return u.sketch_origin.xyz + u.sketch_x.xyz * at.x + u.sketch_y.xyz * at.y;
}

// `logical` pixels from the viewport's top left, y down, in clip space.
fn from_screen(logical: vec2<f32>) -> vec4<f32> {
    let pixels = logical * u.viewport.z - 0.5 * u.viewport.xy;
    return vec4<f32>(vec2<f32>(pixels.x, -pixels.y) / (0.5 * u.viewport.xy), 0.0, 1.0);
}

// Where `at`, a sketch point, is in the world, or with WORLD in `flags`
// the world point `at` with `z`.
fn sketch_world(at: vec2<f32>, z: f32, flags: u32) -> vec3<f32> {
    if (flags & WORLD) != 0u {
        return vec3<f32>(at, z);
    }
    return on_plane(at);
}

// The clip position of `at`, a sketch point, or a world point with WORLD
// in `flags` and `z`, or with SCREEN logical pixels on the screen.
fn sketch_clip(at: vec2<f32>, z: f32, flags: u32) -> vec4<f32> {
    if (flags & SCREEN) != 0u {
        return from_screen(at);
    }
    return u.view_proj * vec4<f32>(sketch_world(at, z, flags), 1.0);
}

// Pull of the sketch's depth tested layers towards the camera, in view
// heights, like the edges' (`pulled`), so a face they lie on doesn't hide
// them while one in front does.
const OVERLAY_PULL: f32 = 0.002;

// The normalized depth of the world point `world`, in front of the near
// plane, for the sketch's depth tested layers: pulled towards the camera
// by OVERLAY_PULL view heights and at least EDGE_DEPTH_BIAS, kept from 0
// to 1 so nothing is cut at the far plane.
fn overlay_depth(world: vec3<f32>) -> f32 {
    let clip = u.view_proj * vec4<f32>(world, 1.0);
    let toward = u.view_proj * vec4<f32>(world + u.backward.xyz * view_height() * OVERLAY_PULL, 1.0);
    // Pulled past the eye, it's as near as can be.
    let pulled = select(0.0, toward.z / toward.w, toward.w > 0.0);
    return clamp(min(pulled, clip.z / clip.w - EDGE_DEPTH_BIAS), 0.0, 1.0);
}

@vertex
fn vs_sketch_line(
    @builtin(vertex_index) index: u32,
    @location(0) ends: vec4<f32>,
    @location(1) neighbours: vec4<f32>,
    @location(2) z: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) style: vec4<f32>,
    @location(5) along: vec2<f32>,
    @location(6) flags: u32,
) -> LineOut {
    let ca = sketch_clip(ends.xy, z.x, flags);
    let cb = sketch_clip(ends.zw, z.y, flags);
    let half = style.x * 0.5 * u.viewport.z;
    let margin = half + 2.0;
    let cut = (flags & SCREEN) == 0u;
    // Each segment is cut the way its neighbours cut it as theirs.
    var prev = none();
    if (flags & HAS_PREV) != 0u {
        prev = shown(sketch_clip(neighbours.xy, z.z, flags), ca, cut, margin);
    }
    var next = none();
    if (flags & HAS_NEXT) != 0u {
        next = shown(cb, sketch_clip(neighbours.zw, z.w, flags), cut, margin);
    }
    let own = shown(ca, cb, cut, margin);
    // On top, or depth tested where its ends show: clip space is linear in
    // the world, so they're as far along in both.
    var depth = vec2<f32>(0.0);
    if SKETCH_DEPTH && cut && own.t.x <= own.t.y {
        let a = sketch_world(ends.xy, z.x, flags);
        let b = sketch_world(ends.zw, z.y, flags);
        depth = vec2<f32>(overlay_depth(mix(a, b, own.t.x)), overlay_depth(mix(a, b, own.t.y)));
    }
    // A sketch's axis fades out pointing at the camera, as the grid's do.
    var faded = color;
    if (flags & FADES) != 0u && cut {
        let along_axis = sketch_world(ends.zw, z.y, flags) - sketch_world(ends.xy, z.x, flags);
        faded.a *= facing(normalize(along_axis));
    }
    // Dashes are in logical pixels, and run along the polyline by its
    // length on the screen: in logical pixels, or in sketch units at the
    // scale of the target, a sketch being looked at straight on.
    let scale = select(u.viewport.y / view_height(), u.viewport.z, (flags & SCREEN) != 0u);
    return line_vertex(index, own, prev, next, flags, depth, half,
        faded, style.yz * u.viewport.z, along * scale);
}

// How far `p` is from the segment from `a` to `b`, and where along it the
// nearest point is, as a fraction of the way from `a`.
fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let ab = b - a;
    let length = dot(ab, ab);
    let t = select(0.0, clamp(dot(p - a, ab) / length, 0.0, 1.0), length > 0.0);
    return vec2<f32>(distance(p, a + ab * t), t);
}

// Where the fragment at `position` is in pixels from the viewport's centre,
// y up, like `to_pixels`.
fn fragment_pixels(position: vec4<f32>) -> vec2<f32> {
    let p = position.xy - u.viewport_origin.xy - 0.5 * u.viewport.xy;
    return vec2<f32>(p.x, -p.y);
}

// How much of the way from 0 to `x` along a line is in its dashes, `on`
// long every `period`, from 0.
fn dashed_to(x: f32, on: f32, period: f32) -> f32 {
    let periods = floor(x / period);
    return periods * on + min(x - periods * period, on);
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return line_color(in, segment_distance(fragment_pixels(in.position), in.ends.xy, in.ends.zw));
}

// What `fs_line` draws, a pixel `own` from the segment (its distance and
// how far along it is).
fn line_color(in: LineOut, own: vec2<f32>) -> vec4<f32> {
    let p = fragment_pixels(in.position);
    // A pixel as near the segment before as this one is that one's, and
    // one nearer the segment after is that one's.
    if (in.flags & HAS_PREV) != 0u && segment_distance(p, in.prev.xy, in.prev.zw).x <= own.x {
        discard;
    }
    if (in.flags & HAS_NEXT) != 0u && segment_distance(p, in.next.xy, in.next.zw).x < own.x {
        discard;
    }
    var coverage = clamp(in.style.x + 0.5 - own.x, 0.0, 1.0);
    // A hollow line leaves out its middle, `style.w` either side.
    if in.style.w > 0.0 {
        coverage -= clamp(in.style.w + 0.5 - own.x, 0.0, 1.0);
    }
    let on = in.style.y;
    if on > 0.0 {
        let period = on + in.style.z;
        let phase = wrapped(mix(in.along.x, in.along.y, own.y), period);
        // How far along the line a pixel on the screen is: more where it's
        // foreshortened, all of it seen end on. Kept off 0 for the
        // division below.
        let shown = max(distance(in.ends.xy, in.ends.zw), 1e-6);
        let step = abs(in.along.y - in.along.x) / shown;
        let footprint = max(step, period * 1e-4);
        // The dashes box filtered over the pixel's footprint, so dashes
        // shorter than a pixel blur to their average rather than alias.
        let half = 0.5 * footprint;
        coverage *= (dashed_to(phase + half, on, period) - dashed_to(phase - half, on, period))
            / footprint;
    }
    return output(vec4<f32>(in.color.rgb, in.color.a * coverage));
}

// --- Origin marker ---
//
// A ring lying flat in the grid's plane around its origin (the world's, or
// that of the sketch being edited), with a dot at the origin itself, each a
// light core with a dark rim so it reads on any background. One screen-space quad: the ring keeps its size on screen
// at any zoom, its widest RING_RADIUS logical pixels, and turns into an
// ellipse as the plane tilts away, so it shows the plane the grid's axis
// lines run in; the dot is always round. Sizes are in logical pixels,
// scaled to physical ones by `u.viewport.z`. Always drawn on top, since the
// origin often coincides with model corners and edges.
//
// The second instance marks the point the camera orbits (`u.pivot`) the
// same way, but with its ring facing the screen, so it's always round, its
// core in the pivot's colour and faded by its opacity; not drawn where it
// shows within a pixel or so of the origin, whose marker is there.

struct OriginOut {
    @builtin(position) position: vec4<f32>,
    // Where the marker's point shows, in pixels from the viewport's centre.
    @location(0) @interpolate(flat) center: vec2<f32>,
    // The ring's ellipse: the images of the grid's x and y axes, scaled so
    // the ellipse is the unit circle's image, in pixels.
    @location(1) @interpolate(flat) ring_x: vec2<f32>,
    @location(2) @interpolate(flat) ring_y: vec2<f32>,
    // The core's colour, and how opaque the marker is.
    @location(3) @interpolate(flat) core: vec4<f32>,
};

const RING_RADIUS: f32 = 10.0;
// Half the width of the ring's core, and the width of the rim around it
// and the dot's.
const RING_CORE: f32 = 0.75;
const RIM: f32 = 1.0;
const DOT_RADIUS: f32 = 2.5;
// The segments the ring is drawn as.
const RING_SEGMENTS: u32 = 48u;
// How near the origin's image, in logical pixels, the pivot's hides.
const PIVOT_AT_ORIGIN: f32 = 1.5;

fn to_pixels(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy / clip.w * 0.5 * u.viewport.xy;
}

// Depth is forced to the near plane so the marker is never occluded.
fn from_pixels(pixels: vec2<f32>, clip: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(pixels / (0.5 * u.viewport.xy) * clip.w, 0.0, clip.w);
}

// How the image of `origin + t * direction` moves on screen as `t` leaves 0,
// in pixels per unit, `origin` showing at clip coordinates `clip`.
fn image_of(direction: vec3<f32>, clip: vec4<f32>) -> vec2<f32> {
    let d = u.view_proj * vec4<f32>(direction, 0.0);
    return (d.xy - clip.xy / clip.w * d.w) / clip.w * 0.5 * u.viewport.xy;
}

// Corner `index` of a square from -1 to 1 drawn as two triangles.
fn square_corner(index: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    return corners[index];
}

@vertex
fn vs_origin(
    @builtin(vertex_index) index: u32,
    @builtin(instance_index) instance: u32,
) -> OriginOut {
    let pivot = instance == 1u;
    let at = select(u.grid_origin.xyz, u.pivot.xyz, pivot);
    let clip = u.view_proj * vec4<f32>(at, 1.0);
    let origin = u.view_proj * vec4<f32>(u.grid_origin.xyz, 1.0);
    var out: OriginOut;
    // Behind the eye, or a pivot that isn't drawn or shows on the origin:
    // nothing to draw.
    let s = u.viewport.z;
    var hidden = clip.w <= 0.0;
    if pivot && !hidden {
        let on_origin = (origin_shown() & ORIGIN_MARKER) != 0u && origin.w > 0.0
            && distance(to_pixels(clip), to_pixels(origin)) < PIVOT_AT_ORIGIN * s;
        hidden = u.pivot.w <= 0.0 || on_origin;
    }
    if hidden {
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let center = to_pixels(clip);
    // The ellipse's semi-major axis, the larger singular value of the
    // matrix with columns `ex` and `ey`, scaled to RING_RADIUS. The
    // pivot's lies in the screen's plane.
    let ex = select(image_of(u.grid_x.xyz, clip), image_of(u.right.xyz, clip), pivot);
    let ey = select(image_of(u.grid_y.xyz, clip), image_of(u.up.xyz, clip), pivot);
    let sum = dot(ex, ex) + dot(ey, ey);
    let det = ex.x * ey.y - ex.y * ey.x;
    let major = sqrt(0.5 * (sum + sqrt(max(sum * sum - 4.0 * det * det, 0.0))));
    let scale = RING_RADIUS * s / max(major, 1e-30);
    let reach = (RING_RADIUS + RING_CORE + RIM + 1.0) * s;
    out.position = from_pixels(center + square_corner(index) * reach, clip);
    out.center = center;
    out.ring_x = ex * scale;
    out.ring_y = ey * scale;
    out.core = select(vec4<f32>(1.0), vec4<f32>(u.pivot_color.rgb, u.pivot.w), pivot);
    return out;
}

@fragment
fn fs_origin(in: OriginOut) -> @location(0) vec4<f32> {
    let s = u.viewport.z;
    let p = fragment_pixels(in.position) - in.center;
    // The distance to the ring, as a polygon fine enough to look smooth;
    // unlike a distance through the ellipse's inverse, it stays right when
    // the plane is seen edge on and the ellipse is a segment.
    var ring = 1e30;
    let step = 6.283185307 / f32(RING_SEGMENTS);
    var a = in.ring_x;
    for (var i = 1u; i <= RING_SEGMENTS; i++) {
        let angle = step * f32(i);
        let b = in.ring_x * cos(angle) + in.ring_y * sin(angle);
        ring = min(ring, segment_distance(p, a, b).x);
        a = b;
    }
    let dot_distance = length(p) - DOT_RADIUS * s;
    let core = max(
        clamp(RING_CORE * s + 0.5 - ring, 0.0, 1.0),
        clamp(0.5 - dot_distance, 0.0, 1.0),
    );
    let rim = max(
        clamp((RING_CORE + RIM) * s + 0.5 - ring, 0.0, 1.0),
        clamp(RIM * s + 0.5 - dot_distance, 0.0, 1.0),
    );
    let color = mix(u.origin_outline.rgb, in.core.rgb, core / max(rim, 1e-6));
    return output(vec4<f32>(color, rim * in.core.a));
}

// --- Origin planes ---
//
// The XY, XZ and YZ planes, an instance each, as squares on one side of
// their axes (PLANE_SIDES: the octant the default camera looks from, so it
// sees inside), as three faces of a cube cornered at the world's origin, so none crosses another: from PLANE_GAP to 1 of PLANE_REACH view
// heights along their axes, so they keep
// their size on screen at any zoom: faintly filled in the colour of the
// axis they're normal to, with a firmer rim. Depth tested, writing no
// depth.

// As `PLANE_REACH` in renderer.rs.
const PLANE_REACH: f32 = 0.2;
// As `PLANE_GAP` in renderer.rs.
const PLANE_GAP: f32 = 0.08;
// As `PLANE_SIDES` in renderer.rs: the octant the planes fill.
const PLANE_SIDES: vec3<f32> = vec3<f32>(1.0, -1.0, 1.0);
const PLANE_FILL: f32 = 0.1;
const PLANE_RIM: f32 = 0.6;
const PLANE_HOVERED_FILL: f32 = 0.3;
// How wide the rim is, in logical pixels.
const PLANE_RIM_WIDTH: f32 = 1.5;

struct PlaneOut {
    @builtin(position) position: vec4<f32>,
    // Where the pixel is in the square, from -1 to 1 along each axis.
    @location(0) at: vec2<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
    // 1 if it's hovered, else 0.
    @location(2) @interpolate(flat) hovered: f32,
};

@vertex
fn vs_origin_plane(
    @builtin(vertex_index) index: u32,
    @builtin(instance_index) instance: u32,
) -> PlaneOut {
    // Each plane's axes, and the axis it's normal to.
    var xs = array<vec3<f32>, 3>(vec3(1.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0));
    var ys = array<vec3<f32>, 3>(vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, 1.0));
    var normals = array<u32, 3>(2u, 1u, 0u);
    // Instances 3 on draw the plane 3 before them hovered.
    let i = instance % 3u;
    let reach = PLANE_REACH * view_height();
    // Grown past its edges by a couple of pixels (as at the target), so
    // its edges' coverage fades out within the quad: anti-aliased without
    // MSAA.
    let half_side = 0.5 * (1.0 - PLANE_GAP) * reach;
    let pixel = view_height() / max(u.viewport.y, 1.0);
    let grow = 1.0 + max(2.0 * pixel / max(half_side, 1e-30), 0.04);
    let corner = square_corner(index) * grow;
    // From PLANE_GAP to 1 of the reach along each axis.
    let along = mix(vec2<f32>(PLANE_GAP), vec2<f32>(1.0), 0.5 * (corner + 1.0)) * reach;
    let world = (xs[i] * along.x + ys[i] * along.y) * PLANE_SIDES;
    var out: PlaneOut;
    out.position = u.view_proj * vec4<f32>(world, 1.0);
    out.at = corner;
    out.color = u.axes[normals[i]].rgb;
    out.hovered = f32(instance >= 3u);
    return out;
}

@fragment
fn fs_origin_plane(in: PlaneOut) -> @location(0) vec4<f32> {
    // Pixels to the square's edge, on the nearer axis: negative past it,
    // where the grown quad fades out.
    let to_edge = (1.0 - abs(in.at)) / max(fwidth(in.at), vec2<f32>(1e-6));
    let coverage = clamp(0.5 + min(to_edge.x, to_edge.y), 0.0, 1.0);
    let fill = mix(PLANE_FILL, PLANE_HOVERED_FILL, in.hovered);
    let width = mix(PLANE_RIM_WIDTH, 2.0 * PLANE_RIM_WIDTH, in.hovered);
    let edge = clamp(width * u.viewport.z + 0.5 - min(to_edge.x, to_edge.y), 0.0, 1.0);
    let alpha = mix(fill, mix(PLANE_RIM, 1.0, in.hovered), edge) * coverage;
    return output(vec4<f32>(in.color, alpha));
}

// --- The sketch being edited: points and fills ---
//
// Drawn over everything, with its lines (`vs_sketch_line`): fills first,
// then lines, then points. With SKETCH_DEPTH, what isn't on the screen is
// given its depth instead (`overlay_depth`), so the model hides it.

struct PointOut {
    @builtin(position) position: vec4<f32>,
    // Where its centre shows, in pixels.
    @location(0) @interpolate(flat) center: vec2<f32>,
    // Its radius and the rim's width, in physical pixels.
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) rim: vec4<f32>,
    @location(3) @interpolate(flat) fill: vec4<f32>,
    // For `fs_highlight_point`: see `highlight_slope`.
    @location(4) @interpolate(flat) slope: f32,
};

// Whether a disc at clip position `clip`, showing at `center` and
// reaching `reach` pixels from it, is behind the near plane or off the
// screen.
fn disc_hidden(clip: vec4<f32>, center: vec2<f32>, reach: f32) -> bool {
    return (perspective() && clip.w < near()) || any(abs(center) > 0.5 * u.viewport.xy + reach);
}

// Corner `index` of the square quad `reach` pixels around `center`, at
// normalized depth `z`.
fn disc_corner(index: u32, center: vec2<f32>, reach: f32, z: f32) -> vec4<f32> {
    let pixels = center + square_corner(index) * reach;
    return vec4<f32>(pixels / (0.5 * u.viewport.xy), z, 1.0);
}

// A point of the sketch as a disc with a rim, a pixel wider for
// anti-aliasing, the same size on screen at any zoom. With WORLD in
// `flags`, a world point whose x and y are `at` and z `world_z`.
@vertex
fn vs_point(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) rim: vec4<f32>,
    @location(3) fill: vec4<f32>,
    @location(4) world_z: f32,
    @location(5) flags: u32,
) -> PointOut {
    var out: PointOut;
    let world = sketch_world(at, world_z, flags & WORLD);
    let clip = u.view_proj * vec4<f32>(world, 1.0);
    let center = to_pixels(clip);
    let radius = size.x * u.viewport.z;
    let reach = radius + 1.0;
    if disc_hidden(clip, center, reach) {
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    var z = 0.0;
    if SKETCH_DEPTH {
        z = overlay_depth(world);
    }
    out.position = disc_corner(index, center, reach, z);
    out.center = center;
    out.size = vec2<f32>(radius, size.y * u.viewport.z);
    out.rim = rim;
    out.fill = fill;
    return out;
}

@fragment
fn fs_point(in: PointOut) -> @location(0) vec4<f32> {
    return point_color(in);
}

fn point_color(in: PointOut) -> vec4<f32> {
    let r = distance(fragment_pixels(in.position), in.center);
    let coverage = clamp(in.size.x + 0.5 - r, 0.0, 1.0);
    let inside = clamp(in.size.x - in.size.y + 0.5 - r, 0.0, 1.0);
    let color = mix(in.rim, in.fill, inside);
    return output(vec4<f32>(color.rgb, color.a * coverage));
}

// Flags of a hovered or selected vertex of the model, as in highlight.rs.
const HOVERED: u32 = 1u;
const SELECTED: u32 = 2u;

// A vertex of the model, hovered or selected, drawn as a sketch point is
// (`fs_point`): a disc of radius VERTEX_RADIUS in the edges' colour, or the
// selection's if selected, within a rim HOVER_RIM wide in
// `u.hover_outline` if hovered, else a pixel's in the edges' colour. Depth
// tested and pulled like the edges, at its centre's depth.
@vertex
fn vs_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec3<f32>,
    @location(1) flags: u32,
) -> PointOut {
    return vertex(index, at, flags);
}

// The hovered vertices alone, drawn again over everything.
@vertex
fn vs_vertex_through(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec3<f32>,
    @location(1) flags: u32,
) -> PointOut {
    if (flags & HOVERED) == 0u {
        var out: PointOut;
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    return vertex(index, at, flags);
}

fn vertex(index: u32, at: vec3<f32>, flags: u32) -> PointOut {
    var out: PointOut;
    let clip = pulled(at);
    let center = to_pixels(clip);
    let s = u.viewport.z;
    let hovered = (flags & HOVERED) != 0u;
    let selected = (flags & SELECTED) != 0u;
    let rim = select(select(1.0, SELECTED_RIM, selected), HOVER_RIM, hovered) * s;
    let radius = VERTEX_RADIUS * s + rim;
    let reach = radius + 1.0;
    if clip.w <= 0.0 || disc_hidden(clip, center, reach) {
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    out.position = disc_corner(index, center, reach, clip.z / clip.w);
    out.center = center;
    out.size = vec2<f32>(radius, rim);
    let edge = vec4<f32>(u.edge.rgb, 1.0);
    let selected_rim = vec4<f32>(u.hover_outline.rgb, SELECTED_RIM_ALPHA);
    out.rim = select(select(edge, selected_rim, selected), vec4<f32>(u.hover_outline.rgb, 1.0),
        hovered);
    out.fill = select(edge, selected_edge(), selected);
    out.slope = highlight_slope(at);
    return out;
}

struct FillOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
};

// A corner of a fill's triangle. In sketch coordinates in perspective, its
// depth is only kept within range, so the triangle is cut at the near
// plane and never at the far one, where the fill isn't depth tested. Depth
// tested, it's `overlay_depth` in front of the near plane, and as it is
// behind it, where it's cut.
@vertex
fn vs_fill(
    @location(0) at: vec2<f32>,
    @location(1) flags: u32,
    @location(2) color: vec4<f32>,
    @location(3) world_z: f32,
) -> FillOut {
    var out: FillOut;
    let clip = sketch_clip(at, world_z, flags);
    var z = 0.0;
    if (flags & SCREEN) == 0u {
        if SKETCH_DEPTH {
            z = select(overlay_depth(sketch_world(at, world_z, flags)) * clip.w, clip.z, clip.z < 0.0);
        } else if perspective() {
            z = 0.5 * (clip.w - near());
        }
    }
    out.position = vec4<f32>(clip.xy, z, clip.w);
    out.color = color;
    return out;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    return output(in.color);
}

// --- Error geometry ---
//
// What a failure is about, red within a wide translucent red halo, drawn
// after everything but the sketch being edited. The halo is coverage first, into a target of its
// own whose blending keeps the most drawn at a pixel, so halos overlapping
// (a polyline's joints, a patch and its boundary) don't darken: the
// boundary curves and points at the halo's width, the patches filled
// (`fs_halo_*`, writing the coverage as it is). It's composited over the
// frame once (`fs_error_halo`), then the geometry itself is drawn over it.
// Both are depth tested against the model, where it shows at full strength
// and where it's hidden (another pipeline, `Greater`) at about 40 %: the
// part's alpha (`part.alpha.x`) carries the strength. Curves and points
// are pulled in by their distance from their middle as the highlight's are
// (`highlight_slope`), patches as the edges are (`pulled`).

// The errors' colours, linear: the core's, and the halo's with its alpha.
// `ErrorUniforms` in renderer.rs. In group 0 beside the scene's uniforms
// (iced's device has two bind groups), bound for the halo's composite and
// the core, not for the halo's coverage, which is drawn into the texture
// bound beside it.
struct ErrorColors {
    core: vec4<f32>,
    halo: vec4<f32>,
};
@group(0) @binding(1) var<uniform> errors: ErrorColors;

fn error_color() -> vec3<f32> {
    return errors.core.rgb;
}

// A corner of a patch's triangle, pulled towards the camera like the edges,
// so a face of the model it lies on doesn't hide it. Not one before the near
// plane: `pulled` would keep it at the plane, and the triangle, unclipped,
// would show what's between it and the eye, across the view.
@vertex
fn vs_error_face(in: MeshIn) -> MeshOut {
    var out: MeshOut;
    let unpulled = u.view_proj * vec4<f32>(in.position, 1.0);
    out.position = select(pulled(in.position), unpulled, unpulled.z < 0.0);
    out.world = in.position;
    out.normal = in.normal;
    return out;
}

// A patch, red and lit as faces are, at the strength drawn.
@fragment
fn fs_error_face(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return output(vec4<f32>(shaded(in, front, error_color()), part.alpha.x));
}

// A patch's halo: all of it covered, at the strength drawn.
@fragment
fn fs_halo_face(in: MeshOut) -> @location(0) vec4<f32> {
    return vec4<f32>(part.alpha.x);
}

// A curve, ERROR_EDGE_WIDTH wide in the errors' colour.
@vertex
fn vs_error_line(in: EdgeIn) -> LineOut {
    let half = ERROR_EDGE_WIDTH * 0.5 * u.viewport.z;
    return highlight_segment(in, half, vec4<f32>(error_color(), part.alpha.x));
}

// A curve's halo, ERROR_HALO wider either side.
@vertex
fn vs_error_halo_line(in: EdgeIn) -> LineOut {
    let half = (ERROR_EDGE_WIDTH * 0.5 + ERROR_HALO) * u.viewport.z;
    return highlight_segment(in, half, vec4<f32>(0.0, 0.0, 0.0, part.alpha.x));
}

// The coverage of a line or disc drawn as the highlight's are: the alpha
// they'd blend at, which `output` leaves as it is.
@fragment
fn fs_halo_line(in: LineOut) -> HighlightOut {
    var out = highlight_line(in);
    out.color = vec4<f32>(out.color.a);
    return out;
}

@fragment
fn fs_halo_point(in: PointOut) -> HighlightOut {
    var out = highlight_point(in);
    out.color = vec4<f32>(out.color.a);
    return out;
}

// A disc of radius `radius` physical pixels at the world point `at` in
// `color`, depth tested at its centre's pulled depth, as a vertex is.
fn error_point(index: u32, at: vec3<f32>, radius: f32, color: vec4<f32>) -> PointOut {
    var out: PointOut;
    let clip = pulled(at);
    let center = to_pixels(clip);
    let reach = radius + 1.0;
    if clip.w <= 0.0 || disc_hidden(clip, center, reach) {
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    out.position = disc_corner(index, center, reach, clip.z / clip.w);
    out.center = center;
    out.size = vec2<f32>(radius, 0.0);
    out.rim = color;
    out.fill = color;
    out.slope = highlight_slope(at);
    return out;
}

// A point, a disc of radius ERROR_POINT_RADIUS in the errors' colour.
@vertex
fn vs_error_point(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec3<f32>,
    @location(1) flags: u32,
) -> PointOut {
    let radius = ERROR_POINT_RADIUS * u.viewport.z;
    return error_point(index, at, radius, vec4<f32>(error_color(), part.alpha.x));
}

// A point's halo, ERROR_HALO wider.
@vertex
fn vs_error_halo_point(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec3<f32>,
    @location(1) flags: u32,
) -> PointOut {
    let radius = (ERROR_POINT_RADIUS + ERROR_HALO) * u.viewport.z;
    return error_point(index, at, radius, vec4<f32>(0.0, 0.0, 0.0, part.alpha.x));
}

// The errors' halo's coverage, or the selected faces' pattern's mask, as
// large as the target and read at the pixel's own texel.
@group(0) @binding(2) var coverage: texture_2d<f32>;

// The halo over the frame, once: its colour at its alpha times the
// coverage.
@fragment
fn fs_error_halo(in: FullscreenOut) -> @location(0) vec4<f32> {
    let covered = textureLoad(coverage, vec2<i32>(in.position.xy), 0).r;
    return output(vec4<f32>(errors.halo.rgb, errors.halo.a * covered));
}
