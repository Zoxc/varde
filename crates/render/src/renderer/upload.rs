//! Uploading meshes, lines and instance buffers to the GPU.

use super::*;

/// The largest buffer the renderer relies on, in bytes: WebGPU's default
/// `maxBufferSize`, which the devices iced asks for have natively and on
/// the web. A larger buffer is a validation error, and wgpu panics on those.
pub(super) const MAX_BUFFER_BYTES: usize = 256 << 20;

// Every mesh the kernel allows fits, one buffer per part: positions,
// normals and indices as they are; edges at most one and a half
// `EdgePoint`s per edge vertex (closed polylines get two more), plus one at
// each end.
const _: () = assert!(size_of::<[f32; 3]>() * RenderMesh::MAX_VERTICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<u32>() * RenderMesh::MAX_INDICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<EdgePoint>() * MeshUpload::MAX_POINTS <= MAX_BUFFER_BYTES);
// Lines are uploaded a segment of two points each, fewer than points.
const _: () = assert!(size_of::<Segment>() * RenderLines::MAX_POINTS <= MAX_BUFFER_BYTES);

/// A segment of a line as the GPU takes it: its two ends.
pub(super) type Segment = [[f32; 3]; 2];

/// How many slots the edge stream is bound to: previous, start, end and
/// next point.
pub(super) const EDGE_SLOTS: u32 = 4;

// Vertex buffer offsets are multiples of 4 bytes, and wgpu's GL backend
// (the browser's WebGL2) allows strides up to 255.
const _: () = assert!(size_of::<EdgePoint>() == 20);

pub(super) struct GpuMesh {
    pub(super) positions: wgpu::Buffer,
    pub(super) normals: wgpu::Buffer,
    pub(super) indices: wgpu::Buffer,
    /// The [`EdgePoint`] stream, if there are any edges or wires.
    pub(super) edges: Option<wgpu::Buffer>,
    /// Whether the wires are drawn with the edges: [`Frame::wireframe`].
    pub(super) wireframe: bool,
    /// The edges of the triangles, once a frame asked for them
    /// ([`Frame::tessellation`]), and whether it does.
    pub(super) triangles: Triangles,
    pub(super) tessellation: bool,
    /// The mesh's parts, as [`RenderMesh::parts`] gives them; one for the
    /// whole mesh if it has none.
    pub(super) parts: Vec<GpuPart>,
    /// To fit the depth range to.
    pub(super) bounds: Option<Aabb>,
}

impl GpuMesh {
    /// The indices of face `face` of `mesh`, the mesh uploaded, and the
    /// part it's of, if it has one.
    pub(super) fn face(&self, mesh: &RenderMesh, face: u32) -> Option<(Range<u32>, usize)> {
        let indices = mesh.face_indices(usize::try_from(face).ok()?)?;
        let part = self.parts.partition_point(|part| part.faces.end <= face);
        // The kernel bounds indices well within `u32`.
        let to_u32 = |n| u32::try_from(n).ok();
        let indices = to_u32(indices.start)?..to_u32(indices.end)?;
        (part < self.parts.len()).then_some((indices, part))
    }
}

/// The edges of a mesh's triangles, for [`Frame::tessellation`].
pub(super) enum Triangles {
    /// Not worked out yet.
    Unbuilt,
    /// Not uploaded: they don't fit in a buffer.
    TooLarge,
    /// Each part's edges, in the order of [`GpuMesh::parts`]: their
    /// segments follow one another in `segments`.
    Built {
        segments: wgpu::Buffer,
        parts: Vec<Range<u32>>,
    },
    /// The mesh has none.
    Empty,
}

/// What the renderer keeps of a part of the mesh to draw it on its own.
#[derive(Debug, Clone)]
pub(super) struct GpuPart {
    /// Its faces' ids.
    pub(super) faces: Range<u32>,
    /// Its triangles' indices.
    pub(super) indices: Range<u32>,
    /// Its edges' points in the [`EdgePoint`] stream, neighbour only ones
    /// included. The parts' follow one another from the stream's second
    /// point, and all the parts' wires follow them.
    pub(super) points: Range<u32>,
    /// Its wires' points in the stream, likewise.
    pub(super) wires: Range<u32>,
    /// Its triangles' bounds, to sort parts less than opaque by.
    pub(super) bounds: Option<Aabb>,
}

pub(super) struct GpuLines {
    pub(super) segments: wgpu::Buffer,
    pub(super) segment_count: u32,
    /// To fit the depth range to.
    pub(super) bounds: Option<Aabb>,
}

/// Writes `bytes` to `buffer` and appends them to `written`.
pub(super) fn write(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    bytes: &[u8],
    written: &mut Vec<u8>,
) {
    queue.write_buffer(buffer, 0, bytes);
    written.extend_from_slice(bytes);
}

/// Uploads `mesh`, or nothing if it is empty.
pub(super) fn upload_mesh(
    device: &wgpu::Device,
    mesh: &RenderMesh,
) -> Result<Option<GpuMesh>, PrepareError> {
    if mesh.indices().is_empty() {
        return Ok(None);
    }
    // Meshes are bounded where they're built, but the device's limit can be
    // lower, and a buffer past it is a validation error, which wgpu panics
    // on.
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep every part within `MAX_BUFFER_BYTES`, so
    // this doesn't saturate.
    let bytes = |len: usize, size: usize| len.saturating_mul(size) as u64;
    // Built in the regeneration lane, natively and in the web worker.
    let upload = mesh.upload();
    let edge_points = (!upload.points().is_empty()).then(|| upload.points());
    let largest = [
        bytes(mesh.positions().len(), size_of::<[f32; 3]>()),
        bytes(mesh.indices().len(), size_of::<u32>()),
        bytes(edge_points.map_or(0, <[_]>::len), size_of::<EdgePoint>()),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if largest > limit {
        return Err(PrepareError::MeshTooLarge {
            bytes: largest,
            limit,
        });
    }
    let edges = edge_points.map(|points| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh edges"),
            contents: bytemuck::cast_slice(points),
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    // The kernel's limits keep these well within `u32`.
    let to_u32 = |range: Range<usize>| {
        let at = |n| u32::try_from(n).expect("kernel bounds indices");
        at(range.start)..at(range.end)
    };
    let part = |faces: Range<usize>,
                indices: Range<usize>,
                [points, wires]: [Range<u32>; 2],
                bounds| GpuPart {
        bounds,
        faces: to_u32(faces),
        indices: to_u32(indices),
        points,
        wires,
    };
    // With no edges or wires, none of the parts has points.
    let points = |i: usize| upload.parts().get(i).cloned().unwrap_or([1..1, 1..1]);
    let bounds = |i: usize| upload.bounds().get(i).copied().flatten();
    let mut parts: Vec<GpuPart> = (mesh.parts().enumerate())
        .map(|(i, part_of)| part(part_of.faces, part_of.indices, points(i), bounds(i)))
        .collect();
    if parts.is_empty() {
        let (faces, indices) = (0..mesh.face_count(), 0..mesh.indices().len());
        parts.push(part(faces, indices, [1..1, 1..1], mesh.bounds()));
    }

    Ok(Some(GpuMesh {
        positions: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh positions"),
            contents: bytemuck::cast_slice(mesh.positions()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        // As many as positions, so the same size.
        normals: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh normals"),
            contents: bytemuck::cast_slice(mesh.normals()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        }),
        edges,
        wireframe: false,
        triangles: Triangles::Unbuilt,
        tessellation: false,
        parts,
        bounds: mesh.bounds(),
    }))
}

/// The edges of `mesh`'s triangles, each once in its part of `parts`, as
/// segments uploaded to a buffer. Fails with [`PrepareError::MeshTooLarge`]
/// where they might not fit in one.
pub(super) fn triangle_edges(
    device: &wgpu::Device,
    mesh: &RenderMesh,
    parts: &[GpuPart],
) -> Result<Triangles, PrepareError> {
    // A triangle's three edges, each shared with at most one other in its
    // part: at most as many as its indices.
    let bytes = (mesh.indices().len() as u64).saturating_mul(size_of::<Segment>() as u64);
    let limit = device.limits().max_buffer_size;
    if bytes > limit {
        return Err(PrepareError::MeshTooLarge { bytes, limit });
    }
    let positions = mesh.positions();
    let mut segments: Vec<Segment> = Vec::new();
    let mut ranges = Vec::with_capacity(parts.len());
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for part in parts {
        let indices = &mesh.indices()[part.indices.start as usize..part.indices.end as usize];
        edges.clear();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            for (a, b) in [(a, b), (b, c), (c, a)] {
                edges.push((a.min(b), a.max(b)));
            }
        }
        edges.sort_unstable();
        edges.dedup();
        // The kernel bounds indices well within `u32`.
        let start = u32::try_from(segments.len()).expect("kernel bounds indices");
        segments
            .extend((edges.iter()).map(|&(a, b)| [positions[a as usize], positions[b as usize]]));
        let end = u32::try_from(segments.len()).expect("kernel bounds indices");
        ranges.push(start..end);
    }
    if segments.is_empty() {
        return Ok(Triangles::Empty);
    }
    Ok(Triangles::Built {
        segments: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde triangle edges"),
            contents: bytemuck::cast_slice(&segments),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        parts: ranges,
    })
}

/// Uploads `lines` a segment each, or nothing if there are none. See
/// [`upload_mesh`] for the limit.
pub(super) fn upload_lines(
    device: &wgpu::Device,
    lines: &RenderLines,
) -> Result<Option<GpuLines>, PrepareError> {
    if lines.segment_count() == 0 {
        return Ok(None);
    }
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep this within `MAX_BUFFER_BYTES`.
    let bytes = lines.segment_count().saturating_mul(size_of::<Segment>()) as u64;
    if bytes > limit {
        return Err(PrepareError::LinesTooLarge { bytes, limit });
    }
    let segments: Vec<Segment> = lines
        .polylines()
        .flat_map(|polyline| polyline.windows(2).map(|pair| [pair[0], pair[1]]))
        .collect();
    let segment_count = u32::try_from(segments.len()).expect("kernel bounds points");

    Ok(Some(GpuLines {
        segments: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde sketch lines"),
            contents: bytemuck::cast_slice(&segments),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        segment_count,
        bounds: lines.bounds(),
    }))
}

/// A layer of the sketch being edited on the GPU: a buffer for each kind
/// of thing it draws.
#[derive(Default)]
pub(super) struct SketchBuffers {
    pub(super) lines: Instances,
    pub(super) points: Instances,
    pub(super) fills: Instances,
}

impl SketchBuffers {
    /// Replaces what the buffers hold with `layer`. On failing, holds
    /// nothing.
    pub(super) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SketchLayer,
    ) -> Result<(), PrepareError> {
        let too_large = |bytes, limit| PrepareError::SketchTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.lines)
            .write(d, q, "varde sketch lines", &layer.lines, too_large)
            .and_then(|()| {
                (self.points).write(d, q, "varde sketch points", &layer.points, too_large)
            })
            .and_then(|()| (self.fills).write(d, q, "varde sketch fills", &layer.fills, too_large));
        if written.is_err() {
            *self = SketchBuffers::default();
        }
        written
    }
}

/// A vertex buffer written again and again, kept while what's written fits
/// in it, and how many things it holds now.
#[derive(Default)]
pub(super) struct Instances {
    pub(super) buffer: Option<wgpu::Buffer>,
    pub(super) count: u32,
}

impl Instances {
    /// The smallest buffer made, in bytes, so a layer growing a little at a
    /// time doesn't make a buffer each time.
    const MIN_BYTES: u64 = 4096;

    /// Replaces what it holds with `items`, in a larger buffer if they
    /// don't fit. Past the device's buffer size, holds nothing and fails
    /// with what `too_large` makes of the bytes needed and that size.
    pub(super) fn write<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        items: &[T],
        too_large: fn(u64, u64) -> PrepareError,
    ) -> Result<(), PrepareError> {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        let size = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        let count = u32::try_from(items.len()).ok().filter(|_| size <= limit);
        let Some(count) = count else {
            self.count = 0;
            return Err(too_large(size, limit));
        };
        self.count = count;
        if items.is_empty() {
            return Ok(());
        }
        if self.buffer.as_ref().is_none_or(|b| b.size() < size) {
            // Within the limit, which `size` is.
            let capacity = size
                .checked_next_power_of_two()
                .unwrap_or(size)
                .max(Self::MIN_BYTES)
                .min(limit)
                .max(size);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
        Ok(())
    }

    /// The part of the buffer to draw from, if it holds anything.
    pub(super) fn drawn(&self) -> Option<wgpu::BufferSlice<'_>> {
        Some(self.held()?.slice(..))
    }

    /// The buffer, if it holds anything.
    pub(super) fn held(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref().filter(|_| self.count > 0)
    }
}

/// [`Frame::highlights`] on the GPU: the edges as an [`EdgePoint`] stream,
/// written again as they change, which of its points are of the outlined
/// edges and which of the selected ones, and the vertices.
#[derive(Default)]
pub(super) struct HighlightBuffers {
    pub(super) edges: Instances,
    pub(super) outlined: Range<u32>,
    pub(super) selected: Range<u32>,
    pub(super) second: Range<u32>,
    pub(super) vertices: Instances,
}

impl HighlightBuffers {
    /// Replaces what the buffers hold with `highlights` in `mesh`. On
    /// failing, holds nothing.
    pub(super) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &RenderMesh,
        highlights: &Highlights,
    ) -> Result<(), PrepareError> {
        let built = highlights.build(mesh);
        let too_large = |bytes, limit| PrepareError::HighlightsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.edges)
            .write(d, q, "varde highlighted edges", &built.edges, too_large)
            .and_then(|()| {
                (self.vertices).write(d, q, "varde vertices", &built.vertices, too_large)
            });
        *self = match written {
            Ok(()) => HighlightBuffers {
                outlined: built.outlined,
                selected: built.selected,
                second: built.second,
                ..std::mem::take(self)
            },
            Err(_) => HighlightBuffers::default(),
        };
        written
    }
}

/// [`Frame::errors`] on the GPU, all of them together: the patches'
/// triangles a corner at a time (positions and normals), the curves as an
/// [`EdgePoint`] stream, a polyline each, and the points. Of each kind,
/// those of parts drawn whole come first, then those drawn only as their
/// halo ([`ErrorParts::halo_only`]).
#[derive(Default)]
pub(super) struct ErrorBuffers {
    pub(super) positions: Instances,
    pub(super) normals: Instances,
    pub(super) edges: Instances,
    pub(super) points: Instances,
    /// How many of each kind are drawn whole, see [`Cores`].
    pub(super) cores: Cores,
    /// To fit the depth range to.
    pub(super) bounds: Option<Aabb>,
}

impl ErrorBuffers {
    /// Replaces what the buffers hold with `errors`. On failing, holds
    /// nothing.
    pub(super) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        errors: &[ErrorParts<'_>],
    ) -> Result<(), PrepareError> {
        let built = BuiltErrors::new(errors);
        let too_large = |bytes, limit| PrepareError::ErrorsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.positions)
            .write(d, q, "varde error faces", &built.positions, too_large)
            .and_then(|()| {
                (self.normals).write(d, q, "varde error normals", &built.normals, too_large)
            })
            .and_then(|()| (self.edges).write(d, q, "varde error lines", &built.edges, too_large))
            .and_then(|()| {
                (self.points).write(d, q, "varde error points", &built.points, too_large)
            });
        *self = match written {
            Ok(()) => ErrorBuffers {
                bounds: built.bounds,
                cores: built.cores,
                ..std::mem::take(self)
            },
            Err(_) => ErrorBuffers::default(),
        };
        written
    }

    /// Whether there's anything to draw.
    pub(super) fn any(&self) -> bool {
        [&self.positions, &self.edges, &self.points]
            .iter()
            .any(|buffer| buffer.held().is_some())
    }
}

/// How many of the errors' corners, [`EdgePoint`]s and points are of
/// parts drawn whole, rather than only as their halo: the corners and
/// points `0..`, the stream's points `1..edges`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Cores {
    pub(super) corners: u32,
    pub(super) edges: u32,
    pub(super) points: u32,
}

/// [`Frame::errors`] as the renderer draws them, see [`ErrorBuffers`].
pub(super) struct BuiltErrors {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) edges: Vec<EdgePoint>,
    pub(super) points: Vec<VertexInstance>,
    pub(super) cores: Cores,
    pub(super) bounds: Option<Aabb>,
}

impl BuiltErrors {
    pub(super) fn new(errors: &[ErrorParts<'_>]) -> BuiltErrors {
        let mut built = BuiltErrors {
            positions: Vec::new(),
            normals: Vec::new(),
            edges: Vec::new(),
            points: Vec::new(),
            cores: Cores::default(),
            bounds: None,
        };
        let mut stream = EdgeStream::with_capacity(0);
        // Each polyline a number of its own, so none joins the next; past
        // `CREASE` they start again, which only the next must differ from.
        let mut polyline = 0u32;
        // Those drawn whole first, then those drawn as their halo alone,
        // counting the first.
        let mut ordered: Vec<&ErrorParts<'_>> = errors.iter().collect();
        ordered.sort_by_key(|error| error.halo_only);
        let mut counted = false;
        for error in ordered {
            if error.halo_only && !counted {
                built.cores = built.counts(&stream);
                counted = true;
            }
            let mesh = error.mesh;
            let (positions, normals) = (mesh.positions(), mesh.normals());
            // A corner at a time: the kernel's meshes index their own
            // positions and normals, in whole triangles.
            for &i in mesh.indices() {
                built.positions.push(positions[i as usize]);
                built.normals.push(normals[i as usize]);
            }
            for line in error.lines.polylines() {
                stream.push_points(polyline, line.iter().copied());
                polyline = (polyline + 1) % CREASE;
            }
            let mut boxes: Vec<Aabb> = [mesh.bounds(), error.lines.bounds()]
                .into_iter()
                .flatten()
                .collect();
            let shown =
                |point: &&[f32; 3]| (point.iter()).all(|x| x.abs() <= RenderLines::MAX_POSITION);
            for &position in error.points.iter().filter(shown) {
                built.points.push(VertexInstance { position, flags: 0 });
                let at = Vec3::from(position);
                boxes.push(Aabb { min: at, max: at });
            }
            built.bounds = (boxes.into_iter().chain(built.bounds)).reduce(|a, b| Aabb {
                min: a.min.min(b.min),
                max: a.max.max(b.max),
            });
        }
        if !counted {
            built.cores = built.counts(&stream);
        }
        // Without a curve, nothing rather than the stream's two ends, so
        // that errors of none don't count as some to draw.
        if stream.len() > 1 {
            built.edges = stream.finish();
        } else {
            built.cores.edges = 0;
        }
        built
    }

    /// How many corners, stream points and points are built so far.
    pub(super) fn counts(&self, stream: &EdgeStream) -> Cores {
        // Past `u32`, writing them fails, and nothing is drawn.
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Cores {
            corners: count(self.positions.len()),
            edges: stream.len(),
            points: count(self.points.len()),
        }
    }
}
