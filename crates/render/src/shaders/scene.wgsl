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
    grid_origin: vec4<f32>,
    grid_x: vec4<f32>,
    grid_y: vec4<f32>,
    grid_axes: array<vec4<f32>, 2>,
    sketch_origin: vec4<f32>,
    sketch_x: vec4<f32>,
    sketch_y: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// Set by the renderer. True if the target stores output as is, so it must be
// sRGB encoded here, false if the target encodes it.
override ENCODE_SRGB: bool;
// Grid lines fade out within this many view heights of the target. The
// depth range, fitted in scene.rs, covers them.
override GRID_FADE_HEIGHTS: f32;
// How wide sketch lines are, in logical pixels.
override LINE_WIDTH: f32;
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

    // The axis lines over the grid, not faded, the y axis over the x axis.
    let pixel_at = fragment_pixels(in.position);
    var axes = array<vec2<f32>, 2>(
        axis_line(pixel_at, origin, dir, u.grid_x.xyz),
        axis_line(pixel_at, origin, dir, u.grid_y.xyz),
    );
    var color = u.grid.rgb;
    var alpha = lines * fade * valid;
    var out_depth = clamp(depth, 0.0, 1.0);
    for (var i = 0; i < 2; i++) {
        let line = axes[i];
        if line.x > 0.0 {
            let a = line.x + alpha * (1.0 - line.x);
            color = (u.grid_axes[i].rgb * line.x + color * alpha * (1.0 - line.x)) / a;
            alpha = a;
            out_depth = line.y;
        }
    }

    var out: GridOut;
    out.color = output(vec4<f32>(color, alpha));
    out.depth = out_depth;
    return out;
}

// How wide the grid's axis lines are, in logical pixels.
const AXIS_WIDTH: f32 = 1.75;

// The grid's axis line along `axis` (a unit vector in the grid's plane,
// through its origin) at the pixel `p` (from `fragment_pixels`), whose ray
// runs from `origin` along `dir`: its coverage there and its depth. The
// line is infinite and not faded: the coverage is from the pixel's
// distance to the line's image on screen, the homogeneous line through the
// images of a point on it and of its direction, exact at any distance and
// zoom in either projection, so it's anti-aliased without MSAA. The depth
// is the axis's point nearest the ray's, and the line is cut where that
// point is behind the near plane, so the part of the image that's behind
// the eye isn't drawn.
fn axis_line(p: vec2<f32>, origin: vec3<f32>, dir: vec3<f32>, axis: vec3<f32>) -> vec2<f32> {
    // The axis's point nearest the target, so clip coordinates stay small.
    let base = u.grid_origin.xyz + axis * dot(u.focus.xyz - u.grid_origin.xyz, axis);
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

    let half_width = 0.5 * AXIS_WIDTH * u.viewport.z;
    let coverage = clamp(half_width + 0.5 - distance, 0.0, 1.0) * f32(seen && in_front);
    return vec2<f32>(coverage, clamp(q.z / max(q.w, 1e-30), 0.0, 1.0));
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

    // Less than opaque when faded, blended over the background.
    return output(vec4<f32>(base * (ambient + diffuse) + spec, u.model.a));
}

// --- Feature edges ---

// Least normalized depth that edges are pulled in by, well clear of
// Depth32Float precision. The depth range is fitted to the scene, so zooming
// into a large one shrinks the pull by view height below that. WebGPU forbids
// pipeline depth bias on lines.
const EDGE_DEPTH_BIAS: f32 = 1e-5;

// `position` in clip space, pulled towards the camera so that edges and
// lines win the depth test against the faces they lie on. Never before the
// near plane, which a pull from just behind it would clip.
fn pulled(position: vec3<f32>) -> vec4<f32> {
    let offset = u.backward.xyz * view_height() * 0.002;
    let clip = u.view_proj * vec4<f32>(position + offset, 1.0);
    let unpulled = u.view_proj * vec4<f32>(position, 1.0);
    let depth = max(min(clip.z / clip.w, unpulled.z / unpulled.w - EDGE_DEPTH_BIAS), 0.0);
    return vec4<f32>(clip.xy, depth * clip.w, clip.w);
}

@vertex
fn vs_edge(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return pulled(position);
}

@fragment
fn fs_edge() -> @location(0) vec4<f32> {
    // Fainter with the faces when faded.
    return output(vec4<f32>(u.edge.rgb, 0.9 * u.edge.a));
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
// Finished sketches' lines (`vs_line`) are depth tested and pulled like
// feature edges, so a line on a face shows while bodies in front of it hide
// it; they come a segment at a time, without neighbours. The sketch being
// edited (`vs_sketch_line`) is drawn over everything.

// Flags of a segment of the sketch being edited, as in sketch.rs: whether
// it has a neighbour before and after it, and whether it's in logical
// pixels from the viewport's top left, or in the world (x and y with z
// apart), rather than sketch coordinates.
const HAS_PREV: u32 = 1u;
const HAS_NEXT: u32 = 2u;
const SCREEN: u32 = 4u;
const WORLD: u32 = 8u;

struct LineOut {
    @builtin(position) position: vec4<f32>,
    // The segment's start and end.
    @location(0) @interpolate(flat) ends: vec4<f32>,
    // The segments before and after it, where `flags` say there are.
    @location(1) @interpolate(flat) prev: vec4<f32>,
    @location(2) @interpolate(flat) next: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    // x: half the width; y, z: a dash's and a gap's lengths, 0 for a solid
    // line.
    @location(4) @interpolate(flat) style: vec4<f32>,
    // How far along the polyline the segment's start and end are.
    @location(5) @interpolate(flat) along: vec2<f32>,
    // HAS_PREV and HAS_NEXT.
    @location(6) @interpolate(flat) flags: u32,
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
fn shown(a: vec4<f32>, b: vec4<f32>, cut: bool, margin: f32) -> Shown {
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

// Nothing: for a neighbour there isn't.
fn none() -> Shown {
    var out: Shown;
    out.t = vec2<f32>(1.0, 0.0);
    return out;
}

@vertex
fn vs_line(
    @builtin(vertex_index) index: u32,
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
) -> LineOut {
    // Cut to the near plane, where clip space z is 0, before pulling, which
    // needs the depth of a point in front of the eye. Clip space is linear
    // in world space, so the cut is where the ends' z mix to 0.
    let za = (u.view_proj * vec4<f32>(start, 1.0)).z;
    let zb = (u.view_proj * vec4<f32>(end, 1.0)).z;
    if za < 0.0 && zb < 0.0 {
        return line_vertex(index, none(), none(), none(), 0u, vec2<f32>(0.0), 0.0,
            vec4<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
    }
    var a = start;
    var b = end;
    if za < 0.0 {
        a = mix(start, end, za / (za - zb));
    } else if zb < 0.0 {
        b = mix(end, start, zb / (zb - za));
    }

    let ca = pulled(a);
    let cb = pulled(b);
    let half = LINE_WIDTH * 0.5 * u.viewport.z;
    // Already cut, before pulling.
    let own = shown(ca, cb, false, half + 2.0);
    let depth = mix(vec2<f32>(ca.z / ca.w), vec2<f32>(cb.z / cb.w), own.t);
    return line_vertex(index, own, none(), none(), 0u, depth, half,
        vec4<f32>(u.sketch.rgb, 1.0), vec2<f32>(0.0), vec2<f32>(0.0));
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
    // Dashes are in logical pixels, and run along the polyline by its
    // length on the screen: in logical pixels, or in sketch units at the
    // scale of the target, a sketch being looked at straight on.
    let scale = select(u.viewport.y / view_height(), u.viewport.z, (flags & SCREEN) != 0u);
    return line_vertex(index, own, prev, next, flags, depth, half,
        color, style.yz * u.viewport.z, along * scale);
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

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let p = fragment_pixels(in.position);
    let own = segment_distance(p, in.ends.xy, in.ends.zw);
    // A pixel as near the segment before as this one is that one's, and
    // one nearer the segment after is that one's.
    if (in.flags & HAS_PREV) != 0u && segment_distance(p, in.prev.xy, in.prev.zw).x <= own.x {
        discard;
    }
    if (in.flags & HAS_NEXT) != 0u && segment_distance(p, in.next.xy, in.next.zw).x < own.x {
        discard;
    }
    var coverage = clamp(in.style.x + 0.5 - own.x, 0.0, 1.0);
    let on = in.style.y;
    if on > 0.0 {
        let period = on + in.style.z;
        let s = mix(in.along.x, in.along.y, own.y);
        let phase = s - floor(s / period) * period;
        // How far inside the dash it is, or outside it, negative, counting
        // the next one's start.
        let inside = max(min(phase, on - phase), phase - period);
        coverage *= clamp(inside + 0.5, 0.0, 1.0);
    }
    return output(vec4<f32>(in.color.rgb, in.color.a * coverage));
}

// --- Origin marker ---
//
// A ring lying flat in the grid's plane around the world origin, with a dot
// at the origin itself, each a light core with a dark rim so it reads on
// any background. One screen-space quad: the ring keeps its size on screen
// at any zoom, its widest RING_RADIUS logical pixels, and turns into an
// ellipse as the plane tilts away, so it shows the plane the grid's axis
// lines run in; the dot is always round. Sizes are in logical pixels,
// scaled to physical ones by `u.viewport.z`. Always drawn on top, since the
// origin often coincides with model corners and edges.

struct OriginOut {
    @builtin(position) position: vec4<f32>,
    // Where the origin shows, in pixels from the viewport's centre.
    @location(0) @interpolate(flat) center: vec2<f32>,
    // The ring's ellipse: the images of the grid's x and y axes, scaled so
    // the ellipse is the unit circle's image, in pixels.
    @location(1) @interpolate(flat) ring_x: vec2<f32>,
    @location(2) @interpolate(flat) ring_y: vec2<f32>,
};

const RING_RADIUS: f32 = 10.0;
// Half the width of the ring's core, and the width of the rim around it
// and the dot's.
const RING_CORE: f32 = 0.75;
const RIM: f32 = 1.0;
const DOT_RADIUS: f32 = 2.5;
// The segments the ring is drawn as.
const RING_SEGMENTS: u32 = 48u;

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

@vertex
fn vs_origin(@builtin(vertex_index) index: u32) -> OriginOut {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let origin = u.view_proj * vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var out: OriginOut;
    if origin.w <= 0.0 {
        // Behind the eye: nothing to draw.
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let s = u.viewport.z;
    let center = to_pixels(origin);
    // The ellipse's semi-major axis, the larger singular value of the
    // matrix with columns `ex` and `ey`, scaled to RING_RADIUS.
    let ex = image_of(u.grid_x.xyz, origin);
    let ey = image_of(u.grid_y.xyz, origin);
    let sum = dot(ex, ex) + dot(ey, ey);
    let det = ex.x * ey.y - ex.y * ey.x;
    let major = sqrt(0.5 * (sum + sqrt(max(sum * sum - 4.0 * det * det, 0.0))));
    let scale = RING_RADIUS * s / max(major, 1e-30);
    let reach = (RING_RADIUS + RING_CORE + RIM + 1.0) * s;
    out.position = from_pixels(center + corners[index] * reach, origin);
    out.center = center;
    out.ring_x = ex * scale;
    out.ring_y = ey * scale;
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
    let color = mix(u.origin_outline.rgb, vec3<f32>(1.0), core / max(rim, 1e-6));
    return output(vec4<f32>(color, rim));
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
};

// A point of the sketch as a disc with a rim, a pixel wider for
// anti-aliasing, the same size on screen at any zoom.
@vertex
fn vs_point(
    @builtin(vertex_index) index: u32,
    @location(0) at: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) rim: vec4<f32>,
    @location(3) fill: vec4<f32>,
) -> PointOut {
    var out: PointOut;
    let clip = u.view_proj * vec4<f32>(on_plane(at), 1.0);
    let center = to_pixels(clip);
    let radius = size.x * u.viewport.z;
    let reach = radius + 1.0;
    // Behind the near plane, or off the screen.
    if (perspective() && clip.w < near())
        || any(abs(center) > 0.5 * u.viewport.xy + reach) {
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let pixels = center + corners[index] * reach;
    var z = 0.0;
    if SKETCH_DEPTH {
        z = overlay_depth(on_plane(at));
    }
    out.position = vec4<f32>(pixels / (0.5 * u.viewport.xy), z, 1.0);
    out.center = center;
    out.size = vec2<f32>(radius, size.y * u.viewport.z);
    out.rim = rim;
    out.fill = fill;
    return out;
}

@fragment
fn fs_point(in: PointOut) -> @location(0) vec4<f32> {
    let r = distance(fragment_pixels(in.position), in.center);
    let coverage = clamp(in.size.x + 0.5 - r, 0.0, 1.0);
    let inside = clamp(in.size.x - in.size.y + 0.5 - r, 0.0, 1.0);
    let color = mix(in.rim, in.fill, inside);
    return output(vec4<f32>(color.rgb, color.a * coverage));
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
