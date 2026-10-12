//! Recording a prepared slot's draw commands.

use super::*;

/// The step of a table of alphas whose last step, `opaque`, is opaque,
/// nearest the product of steps `a`'s and `b`'s alphas.
pub(super) fn product_step(a: u32, b: u32, opaque: u32) -> u32 {
    if opaque == 0 {
        return 0;
    }
    let product = u64::from(a) * u64::from(b) + u64::from(opaque / 2);
    // At most `opaque` for steps within the table.
    u32::try_from(product / u64::from(opaque)).map_or(opaque, |step| step.min(opaque))
}

/// Binds `mesh`'s buffers for drawing its faces.
pub(super) fn bind_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh) {
    pass.set_vertex_buffer(0, mesh.positions.slice(..));
    pass.set_vertex_buffer(1, mesh.normals.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
}

/// The range from `of` the first of `parts` to `of` the last, which
/// follow one another; empty if there are none.
pub(super) fn span(
    mesh: &GpuMesh,
    parts: Range<usize>,
    of: fn(&GpuPart) -> &Range<u32>,
) -> Range<u32> {
    let parts = &mesh.parts[parts];
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => of(first).start..of(last).end,
        _ => 0..0,
    }
}

/// Records drawing the faces of `mesh`'s `parts`, which follow one another,
/// with the pipeline set and its buffers bound ([`bind_faces`]).
pub(super) fn draw_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, parts: Range<usize>) {
    let indices = span(mesh, parts, |part| &part.indices);
    if !indices.is_empty() {
        pass.draw_indexed(indices, 0, 0..1);
    }
}

/// Records drawing the feature edges of `mesh`'s `parts`, which follow one
/// another, with `pipeline`: their points follow one another in the
/// stream, so [`draw_stream`] draws them at once; and their wires likewise
/// in a wireframe.
pub(super) fn draw_edges(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    mesh: &GpuMesh,
    parts: Range<usize>,
) {
    let Some(edges) = &mesh.edges else { return };
    let points = span(mesh, parts.clone(), |part| &part.points);
    draw_stream(pass, pipeline, edges, points);
    if mesh.wireframe {
        draw_stream(pass, pipeline, edges, span(mesh, parts, |part| &part.wires));
    }
}

/// Records drawing the polylines of the [`EdgePoint`] stream in `edges`
/// whose points are `points`, with `pipeline`, an instance per segment.
/// The points either side must be there, and of other polylines or of
/// none, so no segment joins them.
pub(super) fn draw_stream(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    edges: &wgpu::Buffer,
    points: Range<u32>,
) {
    // A segment needs two points; and there's a point before them.
    if points.start == 0 || points.end.saturating_sub(points.start) < 2 {
        return;
    }
    pass.set_pipeline(pipeline);
    let stride = size_of::<EdgePoint>() as u64;
    let from = u64::from(points.start - 1);
    for slot in 0..EDGE_SLOTS {
        pass.set_vertex_buffer(slot, edges.slice((from + u64::from(slot)) * stride..));
    }
    pass.draw(0..LINE_VERTICES, 0..points.end - points.start - 1);
}

/// Whether `slot` has hovered or selected faces to draw where hidden,
/// whose pattern is edged.
pub(super) fn patterned(slot: &Slot) -> bool {
    let drawn = |f: &FaceDraw| match f.tint {
        Tint::Selected => true,
        Tint::Hovered => !slot.state.hover_through,
        Tint::Second => false,
    };
    slot.mesh.is_some() && (slot.state.faces.iter()).any(|f| drawn(f) && !f.indices.is_empty())
}

/// The box the origin planes [`OriginShown`] has drawn take, seen with
/// `camera`, if it has any: see [`PLANE_REACH`].
pub(super) fn planes_bounds(camera: &Camera, origin: OriginShown) -> Option<Aabb> {
    let reach = PLANE_REACH * camera.view_height();
    (origin.planes.contains(&true) && reach.is_finite()).then(|| Aabb {
        min: Vec3::splat(-reach),
        max: Vec3::splat(reach),
    })
}

/// Which world axis `axis`, a unit vector, lies along, 0 to 2, or 3 if
/// none, for its axis line's colour: of [`Colors::axes`], or the grid's.
pub(super) fn axis_index(axis: Vec3) -> f32 {
    let along = axis.abs();
    (0..3)
        .find(|&i| along[i] > 1.0 - 1e-6)
        .map_or(3.0, |i| i as f32)
}

impl Renderer {
    /// Binds `slot`'s group 0 with `part`'s colour, the model's if it's
    /// none of the mesh's parts.
    pub(super) fn set_tint(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, part: usize) {
        let entry = slot.part_tints.get(part).copied().unwrap_or(0);
        pass.set_bind_group(0, &slot.bind_group, &[entry * self.tint_stride]);
    }

    /// Records draw commands for the frame last prepared into `slot` into
    /// `encoder`, compositing over `target`, within `clip`. The order of
    /// the draws, and why, is in `agents/viewport.md`.
    pub fn render(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
    ) {
        self.record(slot, encoder, target, clip, true);
    }

    /// Records the frame last prepared into `slot` as [`Self::render`]
    /// does, with the background, the grid and the origin and pivot
    /// markers only if `backdrop`: without, the model and the finished
    /// sketches alone, over what `target` holds (a preview's, see
    /// `preview.rs`).
    pub(crate) fn record(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
        backdrop: bool,
    ) {
        let Some(depth) = &slot.depth else { return };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let mut drawn = slot.drawn.lock().unwrap_or_else(|e| e.into_inner());
        // Drawn already: composited again alone. A preview's, without
        // the backdrop, is always drawn.
        let fresh = !(backdrop && *drawn == Some(clip));
        *drawn = backdrop.then_some(clip);
        drop(drawn);
        if fresh {
            self.record_scene(slot, encoder, depth, clip, backdrop);
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("varde composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &depth.composite, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Records drawing `slot`'s scene into `depth`'s resolved colour, as
    /// [`Self::record`] says.
    pub(super) fn record_scene(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        depth: &DepthTarget,
        clip: ClipRect,
        backdrop: bool,
    ) {
        // The errors are drawn in passes of their own after the scene's,
        // depth tested against the model, so its depth is kept for them,
        // and the sketch being edited after them, so they don't hide it.
        let errors = slot.error_target.as_ref().filter(|_| slot.errors.any());
        // So is the selected faces' pattern's mask, before what's drawn
        // over everything of the model.
        let pattern = slot.pattern_target.as_ref().filter(|_| patterned(slot));
        let depth_store = if errors.is_some() || pattern.is_some() {
            wgpu::StoreOp::Store
        } else {
            wgpu::StoreOp::Discard
        };
        // The scene is drawn over transparent, into a target of its own
        // (multisampled, see `sample_count`), and composited over `target`
        // last, premultiplied: blending over transparent leaves it so.
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut pass = self.begin(
            slot,
            encoder,
            ("varde scene", depth.color(), clear, depth_store),
            depth,
            (wgpu::LoadOp::Clear(1.0), depth_store),
            clip,
        );
        self.draw_scene(&mut pass, slot, backdrop);
        if let Some(target) = pattern {
            drop(pass);
            self.draw_pattern(slot, encoder, depth, target, clip);
            let load = (wgpu::LoadOp::Load, depth_store);
            let color = (
                "varde scene",
                depth.color(),
                wgpu::LoadOp::Load,
                wgpu::StoreOp::Store,
            );
            pass = self.begin(slot, encoder, color, depth, load, clip);
        }
        self.draw_scene_top(&mut pass, slot, backdrop, pattern);
        if let Some(errors) = errors {
            drop(pass);
            self.draw_errors(slot, encoder, depth, errors, clip);
        } else {
            self.draw_sketch(&mut pass, slot);
            drop(pass);
        }
    }

    /// Begins a pass of `slot`'s frame drawing into the colour target
    /// `target` (its label, view and what that resolves to, load and
    /// store) and `depth`, its depth loaded
    /// and stored as `depth_ops` say (the stencil cleared, never kept),
    /// within `clip`, with the uniforms bound and parts drawn opaque.
    pub(super) fn begin<'p>(
        &self,
        slot: &Slot,
        encoder: &'p mut wgpu::CommandEncoder,
        (label, (target, resolve), load, store): (
            &str,
            (&wgpu::TextureView, Option<&wgpu::TextureView>),
            wgpu::LoadOp<wgpu::Color>,
            wgpu::StoreOp,
        ),
        depth: &DepthTarget,
        (depth_load, depth_store): (wgpu::LoadOp<f32>, wgpu::StoreOp),
        clip: ClipRect,
    ) -> wgpu::RenderPass<'p> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations { load, store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: depth_store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let vp = slot.state.viewport;
        pass.set_viewport(vp.x, vp.y, vp.width, vp.height, 0.0, 1.0);
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_bind_group(0, &slot.bind_group, &[0]);
        self.alphas.set(&mut pass, self.alphas.opaque);
        pass
    }

    /// Records drawing `slot`'s hovered and selected faces' pattern where
    /// the model hides them into `pattern`'s mask, as [`Self::draw_hidden_picks`]
    /// draws it, with `depth` holding the model's depth.
    pub(super) fn draw_pattern(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        depth: &DepthTarget,
        pattern: &CoverageTarget,
        clip: ClipRect,
    ) {
        let Some(mesh) = &slot.mesh else { return };
        let target = (
            "varde selected pattern",
            pattern.color(),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            wgpu::StoreOp::Store,
        );
        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Store);
        let mut pass = self.begin(slot, encoder, target, depth, load, clip);
        bind_faces(&mut pass, mesh);
        for pick in self.picks(slot) {
            let both = &self.hovered_selected_face_pattern;
            self.draw_marked_faces(
                &mut pass,
                slot,
                pick,
                &pick.face_shown,
                [&pick.face_pattern, both],
            );
        }
    }

    /// The hidden faces' and edges' pipelines `slot` draws with, in order:
    /// the hover's, unless [`Frame::hover_through`] draws it whole, then
    /// the selection's over it.
    fn picks(&self, slot: &Slot) -> impl Iterator<Item = &PickPipelines> {
        let hover = (!slot.state.hover_through).then_some(&self.hover_pick);
        hover.into_iter().chain([&self.selection_pick])
    }

    /// Records marking `slot`'s faces tinted `pick.tint` with `pick.mark` by
    /// `marking`, then drawing them by `drawn`, in their parts' tints: a
    /// hovered face as the hover's only if it isn't selected too, a
    /// selected one also hovered by `both`.
    fn draw_marked_faces(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: &Slot,
        pick: &PickPipelines,
        marking: &wgpu::RenderPipeline,
        [drawn, both]: [&wgpu::RenderPipeline; 2],
    ) {
        let (tint, mark) = (pick.tint, pick.mark);
        pass.set_stencil_reference(mark);
        let tinted =
            || (slot.state.faces.iter()).filter(|f| f.tint == tint && !f.indices.is_empty());
        pass.set_pipeline(marking);
        for face in tinted() {
            self.alphas.set(pass, face.step);
            pass.draw_indexed(face.indices.clone(), 0, 0..1);
        }
        for face in tinted() {
            let pipeline = match (tint, face.hovered) {
                (Tint::Hovered, true) => continue,
                (Tint::Selected, true) => both,
                _ => drawn,
            };
            pass.set_pipeline(pipeline);
            self.set_tint(pass, slot, face.part);
            self.alphas.set(pass, face.step);
            pass.draw_indexed(face.indices.clone(), 0, 0..1);
        }
    }

    /// Records drawing `slot`'s errors over the scene in `depth`'s colour, with
    /// `depth` holding the model's depth: their halo's coverage into
    /// `error_target`'s, seen and hidden, then composited over the target
    /// once, then the errors themselves, a kind at a time, hidden then
    /// seen; and last the sketch being edited.
    pub(super) fn draw_errors(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        depth: &DepthTarget,
        error_target: &CoverageTarget,
        clip: ClipRect,
    ) {
        let steps = [self.alphas.opaque, self.alphas.step(Some(ERROR_HIDDEN))];
        let errors = &slot.errors;
        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Store);
        let mut pass = self.begin(
            slot,
            encoder,
            (
                "varde error halo",
                error_target.color(),
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                wgpu::StoreOp::Store,
            ),
            depth,
            load,
            clip,
        );
        self.draw_error_layers(&mut pass, &self.errors.halo, steps, errors, None);
        drop(pass);

        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Discard);
        let target = (
            "varde errors",
            depth.color(),
            wgpu::LoadOp::Load,
            wgpu::StoreOp::Store,
        );
        let mut pass = self.begin(slot, encoder, target, depth, load, clip);
        pass.set_pipeline(&self.errors.composite);
        pass.set_bind_group(0, &error_target.group, &[]);
        pass.draw(0..3, 0..1);
        // Hidden first, so what shows of each kind goes over it.
        let [seen, hidden] = &self.errors.core;
        let [opaque, dimmed] = steps;
        let layers = [hidden, seen];
        let cores = Some(errors.cores);
        self.draw_error_layers(&mut pass, layers, [dimmed, opaque], errors, cores);
        self.alphas.set(&mut pass, opaque);
        pass.set_bind_group(0, &slot.bind_group, &[0]);
        self.draw_sketch(&mut pass, slot);
    }

    /// Records drawing `errors` with `layers` at the alphas' steps `steps`,
    /// their faces with each, then their lines, then their points: all of
    /// them, or only `cores`, those of the parts drawn whole.
    pub(super) fn draw_error_layers<'l>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        layers: impl IntoIterator<Item = &'l ErrorLayer> + Clone,
        steps: [u32; 2],
        errors: &ErrorBuffers,
        cores: Option<Cores>,
    ) {
        let with_steps = || layers.clone().into_iter().zip(steps);
        // Between the stream's two ends.
        let all = Cores {
            corners: errors.positions.count,
            edges: errors.edges.count.saturating_sub(1),
            points: errors.points.count,
        };
        let drawn = cores.unwrap_or(all);
        if let (Some(positions), Some(normals)) = (errors.positions.drawn(), errors.normals.drawn())
            && drawn.corners > 0
        {
            pass.set_vertex_buffer(0, positions);
            pass.set_vertex_buffer(1, normals);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.faces);
                pass.draw(0..drawn.corners, 0..1);
            }
        }
        if let Some(edges) = errors.edges.held() {
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                draw_stream(pass, &layer.lines, edges, 1..drawn.edges);
            }
        }
        if let Some(points) = errors.points.drawn()
            && drawn.points > 0
        {
            pass.set_vertex_buffer(0, points);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.points);
                pass.draw(0..POINT_VERTICES, 0..drawn.points);
            }
        }
    }

    /// Records drawing `slot`'s scene into `pass`: everything but the
    /// errors and the sketch being edited ([`Self::draw_sketch`]), the
    /// backdrop only if `backdrop` (see [`Self::record`]).
    pub(super) fn draw_scene(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, backdrop: bool) {
        if backdrop {
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }

        let mesh = slot.mesh.as_ref();
        let draws = &slot.state.draws;
        if let Some(mesh) = mesh {
            bind_faces(pass, mesh);
            if slot.state.faded {
                let all = 0..mesh.parts.len();
                pass.set_pipeline(&self.mesh_depth);
                draw_faces(pass, mesh, all.clone());
                // Faded, every part is opaque to `draws`, in runs of a
                // colour.
                pass.set_pipeline(&self.mesh_faded);
                for run in &draws.opaque {
                    self.set_tint(pass, slot, run.start);
                    draw_faces(pass, mesh, run.clone());
                }
                pass.set_bind_group(0, &slot.bind_group, &[0]);
                // What a sketch's tool picks of the model, over it.
                self.draw_picked_faces(pass, &slot.state.faces, false);
                draw_edges(pass, &self.edges, mesh, all.clone());
                self.draw_triangle_edges(pass, mesh, all);
            } else {
                pass.set_pipeline(&self.mesh);
                for run in &draws.opaque {
                    self.set_tint(pass, slot, run.start);
                    draw_faces(pass, mesh, run.clone());
                }
                pass.set_bind_group(0, &slot.bind_group, &[0]);
                // Before anything else writes depth where they are.
                self.draw_picked_faces(pass, &slot.state.faces, false);
                for run in &draws.opaque {
                    draw_edges(pass, &self.edges, mesh, run.clone());
                    self.draw_triangle_edges(pass, mesh, run.clone());
                }
            }
        }

        // Drawn after the model so they blend over the background and are
        // occluded by geometry. The sketches go over the grid, since they
        // often lie on it.
        if backdrop {
            pass.set_pipeline(&self.grid);
            pass.set_bind_group(0, &slot.bind_group, &[0]);
            let (step, other) = slot.state.grid_steps;
            self.alphas.set(pass, step);
            pass.draw(0..3, 0..1);
            if let Some(step) = other {
                pass.set_bind_group(0, &slot.other_grid_group, &[0]);
                self.alphas.set(pass, step);
                pass.draw(0..3, 0..1);
                pass.set_bind_group(0, &slot.bind_group, &[0]);
            }
            self.alphas.set(pass, self.alphas.opaque);
        }

        // Under the finished sketches, which often lie on them, unless
        // drawn over the model, below.
        if backdrop && !slot.state.origin.planes_on_top {
            self.draw_origin_planes(pass, slot, &self.origin_planes);
        }

        if let Some(lines) = &slot.lines {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, lines.segments.slice(..));
            pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
        }

        // Only the opaque parts have written depth so far, so the hidden
        // edges here are those they hide; what glass hides is dashed below.
        // Then the glass's visible edges, under the faces drawn next.
        if !slot.state.faded
            && let Some(mesh) = mesh
        {
            if slot.state.hidden_edges {
                for run in &draws.opaque {
                    draw_edges(pass, &self.hidden_edges, mesh, run.clone());
                }
                self.draw_glass_edges(pass, &self.hidden_edges, mesh, draws);
            }
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
        }

        // The extrude's layers under the glass in front of them, if there
        // is any, and so under the origin marker too.
        let glass = slot.has_glass();
        let layers = [&slot.sketch_base, &slot.sketch_live];
        if slot.state.sketching && slot.state.sketch_depth && glass {
            for layer in layers {
                self.sketch_depth_tested.draw(pass, layer);
            }
        }

        // The glass: the hover and selection under it, to show through
        // dimmed; its faces far to near; its depth a part at a time, each
        // followed by the edges it hides, dashed; then its picked faces and
        // its edges again, undimmed on the glass they lie on.
        if let Some(mesh) = mesh.filter(|_| glass) {
            if !slot.state.faded {
                self.draw_highlights(pass, slot);
            }
            bind_faces(pass, mesh);
            pass.set_pipeline(&self.glass_back);
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.set_tint(pass, slot, part);
                draw_faces(pass, mesh, part..part + 1);
            }
            // Each part's front faces over their own depth, so only the
            // nearest are blended, not whichever come last.
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.set_tint(pass, slot, part);
                for pipeline in [&self.mesh_depth, &self.glass_front] {
                    pass.set_pipeline(pipeline);
                    draw_faces(pass, mesh, part..part + 1);
                }
            }
            for (i, &(part, step)) in draws.transparent.iter().enumerate() {
                // Never 0, what the stencil is cleared to, and under
                // `PICK_MARKS`; repeating only past 63 parts less than opaque.
                pass.set_stencil_reference(i as u32 % 63 + 1);
                bind_faces(pass, mesh);
                pass.set_pipeline(&self.glass_depth);
                draw_faces(pass, mesh, part..part + 1);
                if slot.state.hidden_edges {
                    self.draw_hidden_by_glass(pass, mesh, draws, step);
                }
            }
            // What's in front of the glass, drawn under it above, again
            // over it where it's the nearest.
            pass.set_stencil_reference(0);
            pass.set_bind_group(0, &slot.bind_group, &[0]);
            self.alphas.set(pass, self.alphas.opaque);
            if let Some(lines) = &slot.lines {
                pass.set_pipeline(&self.lines_over_glass);
                pass.set_vertex_buffer(0, lines.segments.slice(..));
                pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
            }
            if slot.state.sketching && slot.state.sketch_depth {
                for layer in layers {
                    self.sketch_over_glass.draw(pass, layer);
                }
            }
            bind_faces(pass, mesh);
            self.draw_picked_faces(pass, &slot.state.faces, true);
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                self.draw_triangle_edges(pass, mesh, part..part + 1);
            }
        }

        // The hover and the selection over everything of the model,
        // faded or not.
        if mesh.is_some() {
            self.draw_highlights(pass, slot);
        }
        if let Some(mesh) = mesh.filter(|_| slot.state.hover_through) {
            self.draw_hover_through(pass, slot, mesh);
        }
    }

    /// Records drawing the rest of `slot`'s scene into `pass` after
    /// [`Self::draw_scene`], over everything of the model: what it hides
    /// of the hover and selection, the selection's pattern edged from
    /// `pattern`'s mask if given, and the markers.
    pub(super) fn draw_scene_top(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: &Slot,
        backdrop: bool,
        pattern: Option<&CoverageTarget>,
    ) {
        if let Some(mesh) = &slot.mesh {
            self.draw_hidden_picks(pass, slot, mesh, pattern);
        }

        // Over the model, which can't hide them while one is picked.
        if backdrop && slot.state.origin.planes_on_top {
            self.draw_origin_planes(pass, slot, &self.origin_planes_on_top);
        }

        if backdrop {
            pass.set_pipeline(&self.origin);
            // The pivot's marker is the second instance.
            let first = if slot.state.origin.marker { 0 } else { 1 };
            pass.draw(0..ORIGIN_VERTICES, first..MARKERS);
        }
    }

    /// Records drawing the origin planes `slot` shows with `pipeline`.
    pub(super) fn draw_origin_planes(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: &Slot,
        pipeline: &wgpu::RenderPipeline,
    ) {
        pass.set_pipeline(pipeline);
        // The hovered one's instance is 3 on, see `vs_origin_plane`.
        for (plane, _) in (0..)
            .zip(slot.state.origin.planes)
            .filter(|(_, shown)| *shown)
        {
            let hovered = slot
                .state
                .origin
                .emphasised(OriginPart::Plane(plane as usize));
            let instance = if hovered { plane + 3 } else { plane };
            pass.draw(0..PLANE_VERTICES, instance..instance + 1);
        }
    }

    /// Records drawing the sketch being edited into `pass`, after the
    /// scene and the errors: over everything, the origin marker included,
    /// since its points often lie on it, and the errors, so they don't
    /// hide what's edited; or depth tested, unless [`Self::draw_scene`]
    /// drew it under the glass.
    pub(super) fn draw_sketch(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        if slot.state.sketching && !(slot.state.sketch_depth && slot.has_glass()) {
            let pipelines = if slot.state.sketch_depth {
                &self.sketch_depth_tested
            } else {
                &self.sketch_on_top
            };
            for layer in [&slot.sketch_base, &slot.sketch_live] {
                pipelines.draw(pass, layer);
            }
        }
    }

    /// Records drawing `faces` again over themselves, hovered or selected,
    /// those of parts less than opaque if `transparent`, else the opaque
    /// ones', with the mesh's buffers bound ([`bind_faces`]).
    pub(super) fn draw_picked_faces(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        faces: &[FaceDraw],
        transparent: bool,
    ) {
        for face in faces {
            if (face.step < self.alphas.opaque) != transparent || face.indices.is_empty() {
                continue;
            }
            let pipeline = match face.tint {
                Tint::Hovered => &self.hover_face,
                Tint::Selected => &self.selected_face,
                Tint::Second => &self.second_face,
            };
            pass.set_pipeline(pipeline);
            self.alphas.set(pass, face.step);
            pass.draw_indexed(face.indices.clone(), 0, 0..1);
        }
    }

    /// Records drawing the edges of the triangles of `mesh`'s `parts`,
    /// which follow one another, if [`Frame::tessellation`] asked for them.
    pub(super) fn draw_triangle_edges(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        parts: Range<usize>,
    ) {
        let Triangles::Built {
            segments,
            parts: of,
        } = &mesh.triangles
        else {
            return;
        };
        let (Some(first), Some(last)) = (of.get(parts.start), of.get(parts.end.wrapping_sub(1)))
        else {
            return;
        };
        if !mesh.tessellation || first.start >= last.end {
            return;
        }
        pass.set_pipeline(&self.triangle_edges);
        pass.set_vertex_buffer(0, segments.slice(..));
        pass.draw(0..LINE_VERTICES, first.start..last.end);
    }

    /// Records drawing `slot`'s hovered edges within their outline,
    /// selected edges, and hovered and selected vertices.
    pub(super) fn draw_highlights(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        let highlights = &slot.highlights;
        self.alphas.set(pass, self.alphas.opaque);
        if let Some(edges) = highlights.edges.held() {
            let outlined = highlights.outlined.clone();
            draw_stream(pass, &self.outline, edges, outlined.clone());
            draw_stream(pass, &self.hovered_edges, edges, outlined);
            let selected = highlights.selected.clone();
            draw_stream(pass, &self.selected_outline, edges, selected.clone());
            draw_stream(pass, &self.selected_edges, edges, selected);
            let second = highlights.second.clone();
            draw_stream(pass, &self.second_outline, edges, second.clone());
            draw_stream(pass, &self.second_edges, edges, second);
        }
        if let Some(vertices) = highlights.vertices.drawn() {
            pass.set_pipeline(&self.vertices);
            pass.set_vertex_buffer(0, vertices);
            pass.draw(0..POINT_VERTICES, 0..highlights.vertices.count);
        }
    }

    /// Records drawing what the model hides of `slot`'s hovered and
    /// selected faces and edges over everything: the faces washed and
    /// striped, the edges dashed; the selection over the hover. The hover
    /// not while [`Frame::hover_through`] draws it whole.
    pub(super) fn draw_hidden_picks(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: &Slot,
        mesh: &GpuMesh,
        pattern: Option<&CoverageTarget>,
    ) {
        let highlights = &slot.highlights;
        for pick in self.picks(slot) {
            let drawn = match pick.tint {
                Tint::Hovered => &highlights.outlined,
                _ => &highlights.selected,
            };
            // Marking where the faces are the nearest, then striping
            // where they're hidden, outside that: what they hide of
            // themselves, as a selected body's far side, isn't striped.
            // The stripes are in the colour the face is shown in, so each
            // in its part's tint; a face both hovered and selected is
            // striped by the selection alone, from the hover's colour.
            bind_faces(pass, mesh);
            let both = &self.hovered_selected_face_hidden;
            self.draw_marked_faces(pass, slot, pick, &pick.face_mark, [&pick.face_hidden, both]);
            // The hovered and selected faces' pattern's edge, from its
            // mask, over both.
            if pick.tint == Tint::Selected
                && let Some(pattern) = pattern
            {
                self.alphas.set(pass, self.alphas.opaque);
                pass.set_pipeline(&self.pattern_edge);
                pass.set_bind_group(0, &pattern.group, &[]);
                pass.draw(0..3, 0..1);
            }
            pass.set_bind_group(0, &slot.bind_group, &[0]);
            pass.set_stencil_reference(0);
            self.alphas.set(pass, self.alphas.opaque);
            if let Some(stream) = highlights.edges.held() {
                for pipeline in &pick.edges_hidden {
                    draw_stream(pass, pipeline, stream, drawn.clone());
                }
            }
        }
    }

    /// Records drawing `slot`'s hovered faces, outlined edges and hovered
    /// vertices again over everything: [`Frame::hover_through`].
    pub(super) fn draw_hover_through(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        slot: &Slot,
        mesh: &GpuMesh,
    ) {
        bind_faces(pass, mesh);
        pass.set_pipeline(&self.hover_face_through);
        for face in &slot.state.faces {
            if face.tint == Tint::Hovered && !face.indices.is_empty() {
                self.alphas.set(pass, face.step);
                pass.draw_indexed(face.indices.clone(), 0, 0..1);
            }
        }
        let highlights = &slot.highlights;
        self.alphas.set(pass, self.alphas.opaque);
        if let Some(edges) = highlights.edges.held() {
            let outlined = highlights.outlined.clone();
            draw_stream(pass, &self.outline_through, edges, outlined.clone());
            draw_stream(pass, &self.hovered_edges_through, edges, outlined);
        }
        if let Some(vertices) = highlights.vertices.drawn() {
            pass.set_pipeline(&self.vertices_through);
            pass.set_vertex_buffer(0, vertices);
            pass.draw(0..POINT_VERTICES, 0..highlights.vertices.count);
        }
    }

    /// Records drawing the edges of `mesh`'s parts less than opaque, as
    /// `draws` has them, each at its alpha, with `pipeline`.
    pub(super) fn draw_glass_edges(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        mesh: &GpuMesh,
        draws: &PartDraws,
    ) {
        for &(part, step) in &draws.transparent {
            self.alphas.set(pass, step);
            draw_edges(pass, pipeline, mesh, part..part + 1);
        }
    }

    /// Records drawing, dashed, the edges of every part of `mesh` hidden by
    /// the glass whose depth and stencil reference were just written, at
    /// its alpha's step `step` times their own part's.
    pub(super) fn draw_hidden_by_glass(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        draws: &PartDraws,
        step: u32,
    ) {
        let opaque = self.alphas.opaque;
        let runs = draws.opaque.iter().map(|run| (run.clone(), step));
        let glass = draws
            .transparent
            .iter()
            .map(|&(part, own)| (part..part + 1, product_step(step, own, opaque)));
        for (parts, step) in runs.chain(glass) {
            if step > 0 {
                self.alphas.set(pass, step);
                draw_edges(pass, &self.hidden_by_glass, mesh, parts);
            }
        }
    }
}
