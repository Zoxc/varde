//! Joins merging bodies: a seeded fuzz of blocks bridged by a bar against
//! their exact union, merges followed through edits, deletes, undo and
//! drafts, and where a merge that fails shows.

use super::*;

/// A block from its least to its greatest corner, as
/// `[x0, y0, z0, x1, y1, z1]`.
type Block = [f64; 6];

/// A xorshift generator: the cases are the same on every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..4 {
            rng.next();
        }
        rng
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// One of `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// One of `0..n`, as a float.
    fn step(&mut self, n: u64) -> f64 {
        // Small counts: exact as floats.
        self.below(n) as f64
    }
}

/// The spans along z the blocks take: up from XY, down from it or both,
/// so that they meet flush, along edges or overlap.
const SPANS: [(f64, f64); 6] = [
    (0.0, 10.0),
    (0.0, 5.0),
    (-5.0, 0.0),
    (-5.0, 10.0),
    (-3.0, 0.0),
    (-2.0, 8.0),
];

/// Two to four blocks on a 5 mm grid, some put flush with or corner to
/// corner with the one before, and a bar bridging two of them (each
/// one's middle inside it), off the grid by half a step or more.
fn case(seed: u64) -> (Vec<Block>, Block) {
    let mut rng = Rng::new(seed);
    let count = 2 + rng.below(3) as usize;
    let mut blocks: Vec<Block> = Vec::with_capacity(count);
    for k in 0..count {
        let (w, h) = (5.0 * (1.0 + rng.step(3)), 5.0 * (1.0 + rng.step(3)));
        let (mut x0, mut y0) = (5.0 * rng.step(9), 5.0 * rng.step(9));
        if k > 0 && rng.below(3) == 0 {
            let before = blocks[k - 1];
            x0 = before[3];
            if rng.below(2) == 0 {
                y0 = before[4];
            }
        }
        let (z0, z1) = SPANS[rng.below(SPANS.len() as u64) as usize];
        blocks.push([x0, y0, z0, x0 + w, y0 + h, z1]);
    }
    let i = rng.below(count as u64) as usize;
    let j = (i + 1 + rng.below(count as u64 - 1) as usize) % count;
    let middle = |b: Block| ((b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0);
    let (a, b) = (middle(blocks[i]), middle(blocks[j]));
    let reach = 1.0 + rng.step(3);
    let (z0, z1) = SPANS[rng.below(SPANS.len() as u64) as usize];
    let bar = [
        a.0.min(b.0) - reach,
        a.1.min(b.1) - reach,
        z0,
        a.0.max(b.0) + reach,
        a.1.max(b.1) + reach,
        z1,
    ];
    (blocks, bar)
}

/// The cells of the grid all of `blocks`' faces make, and whether each
/// is inside one of them: by x, then y, then z index.
struct Cells {
    planes: [Vec<f64>; 3],
    filled: Vec<bool>,
}

impl Cells {
    fn new(blocks: &[Block]) -> Cells {
        let planes = [0, 1, 2].map(|axis| {
            let mut at: Vec<f64> = (blocks.iter())
                .flat_map(|block| [block[axis], block[axis + 3]])
                .collect();
            at.sort_by(f64::total_cmp);
            at.dedup();
            at
        });
        let mut filled = Vec::new();
        for i in 0..planes[0].len() - 1 {
            for j in 0..planes[1].len() - 1 {
                for k in 0..planes[2].len() - 1 {
                    let middle = [(0, i), (1, j), (2, k)]
                        .map(|(axis, n)| (planes[axis][n] + planes[axis][n + 1]) / 2.0);
                    filled.push(blocks.iter().any(|block| {
                        (0..3).all(|axis| {
                            block[axis] < middle[axis] && middle[axis] < block[axis + 3]
                        })
                    }));
                }
            }
        }
        Cells { planes, filled }
    }

    fn counts(&self) -> [usize; 3] {
        [0, 1, 2].map(|axis| self.planes[axis].len() - 1)
    }

    /// Whether cell `(i, j, k)` is filled; none outside the grid is.
    fn at(&self, [i, j, k]: [isize; 3]) -> bool {
        let [ni, nj, nk] = self.counts();
        let inside = |n: isize, count: usize| usize::try_from(n).is_ok_and(|n| n < count);
        if !(inside(i, ni) && inside(j, nj) && inside(k, nk)) {
            return false;
        }
        let [i, j, k] = [i, j, k].map(|n| n as usize);
        self.filled[(i * nj + j) * nk + k]
    }

    /// The volume of the union, exactly as far as the sums go.
    fn volume(&self) -> f64 {
        let [ni, nj, nk] = self.counts();
        let size = |axis: usize, n: usize| self.planes[axis][n + 1] - self.planes[axis][n];
        let mut volume = 0.0;
        for i in 0..ni {
            for j in 0..nj {
                for k in 0..nk {
                    if self.filled[(i * nj + j) * nk + k] {
                        volume += size(0, i) * size(1, j) * size(2, k);
                    }
                }
            }
        }
        volume
    }

    /// Whether the union's boundary is a manifold: no two cells, filled
    /// or empty, meet only along an edge or at a corner where the other
    /// kind fills the rest round it.
    fn manifold(&self) -> bool {
        let [ni, nj, nk] = self.counts().map(|n| n as isize);
        for i in 0..=ni {
            for j in 0..=nj {
                for k in 0..=nk {
                    // The eight cells round the corner (i, j, k).
                    let cell = |m: usize| {
                        let bit = |b: usize| ((m >> b) & 1) as isize - 1;
                        self.at([i + bit(0), j + bit(1), k + bit(2)])
                    };
                    for kind in [true, false] {
                        let of = |m: usize| cell(m) == kind;
                        // Round each edge through the corner: the four
                        // cells on one side of it along the edge.
                        for axis in 0..3 {
                            for side in 0..2 {
                                let [a, b] = [(axis + 1) % 3, (axis + 2) % 3];
                                let m = |u: usize, v: usize| (side << axis) | (u << a) | (v << b);
                                let diagonal = of(m(0, 0)) && of(m(1, 1));
                                if diagonal && !of(m(0, 1)) && !of(m(1, 0)) {
                                    return false;
                                }
                            }
                        }
                        // Only two opposite cells of the eight.
                        if (0..8).filter(|&m| of(m)).count() == 2
                            && (0..8).any(|m| of(m) && of(7 - m))
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// Adds `block` to `editor`, extruded from a rectangle on XY with
/// `operation`: its feature.
fn add_block(editor: &mut Editor, block: Block, operation: Operation) -> FeatureId {
    let [x0, y0, z0, x1, y1, z1] = block;
    let document = editor.document();
    let number = |value: f64| length(document, &format!("{value}"));
    let (extent, flip) = match (z0 == 0.0, z1 == 0.0) {
        (true, _) => (Extent::OneSide(number(z1)), false),
        (_, true) => (Extent::OneSide(number(-z0)), true),
        _ => (Extent::TwoSides(number(z1), number(-z0)), false),
    };
    let feature = add_extrude(editor, rectangle((x0, y0), (x1, y1)), extent, operation);
    if flip {
        set_extrude(editor, feature, |extrude| extrude.flip = true);
    }
    feature
}

/// The volume of `block`.
fn block_volume(block: &Block) -> f64 {
    (0..3).map(|axis| block[axis + 3] - block[axis]).product()
}

/// Whether `a` and `b` share some volume, and whether they meet at all.
fn overlap(a: &Block, b: &Block) -> (bool, bool) {
    let apart = |strict: bool| {
        (0..3).any(|axis| {
            let (low, high) = (a[axis].max(b[axis]), a[axis + 3].min(b[axis + 3]));
            if strict { low >= high } else { low > high }
        })
    };
    (!apart(true), !apart(false))
}

/// A join bridging two to four blocks, on many seeds: it comes out as the
/// exact union of the blocks it touches and itself, in the first of them,
/// the others listed as merged into it and the blocks it doesn't touch
/// left as they were; or, where that union isn't a manifold (blocks
/// meeting along an edge or at a corner the bar doesn't cover), it fails
/// and changes nothing.
#[test]
fn a_join_bridging_blocks_is_their_union_or_fails_where_that_isnt_a_solid() {
    let (mut merged, mut refused) = (0, 0);
    for seed in 0..24 {
        let (blocks, bar) = case(seed);
        let mut editor = Editor::new(Document::default());
        for &block in &blocks {
            add_block(&mut editor, block, Operation::NewBody(BodyId::NEW));
        }
        let ids: Vec<BodyId> = editor.document().bodies().iter().map(|b| b.id).collect();
        let join = add_block(&mut editor, bar, Operation::Join(Targets::default()));
        let evaluation = evaluated(editor.document());
        let context = format!("seed {seed}: {blocks:?}, bar {bar:?}");
        let [(feature, touched)] = &evaluation.touched[..] else {
            panic!("{context}: {:?}", evaluation.touched);
        };
        assert_eq!(*feature, join, "{context}");
        // Every block the bar shares volume with is touched, none it
        // doesn't meet is.
        for (block, id) in blocks.iter().zip(&ids) {
            let (shares, meets) = overlap(block, &bar);
            assert!(!shares || touched.contains(id), "{context}: {id:?}");
            assert!(meets || !touched.contains(id), "{context}: {id:?}");
        }
        let block_of = |id: &BodyId| blocks[ids.iter().position(|b| b == id).unwrap()];
        let mut union: Vec<Block> = touched.iter().map(block_of).collect();
        union.push(bar);
        let cells = Cells::new(&union);
        let unchanged = |id: &BodyId| {
            let made = (evaluation.bodies.iter()).find(|made| made.body == *id);
            let made = made.unwrap_or_else(|| panic!("{context}: {id:?} has no solid"));
            assert_near(made.solid.volume(), block_volume(&block_of(id)));
        };
        if !cells.manifold() {
            refused += 1;
            assert_eq!(evaluation.failed.len(), 1, "{context}");
            assert!(evaluation.merged.is_empty(), "{context}");
            // One way past it, unticking a body, where there are others.
            let error = &evaluation.failed[0].message;
            let hints = usize::from(touched.len() > 1);
            assert_eq!(error.matches("untick").count(), hints, "{context}: {error}");
            // Named for what it is, not a guess.
            assert!(error.contains(TOUCHES_ITSELF), "{context}: {error}");
            ids.iter().for_each(unchanged);
            continue;
        }
        assert!(
            evaluation.failed.is_empty(),
            "{context}: {:?}",
            evaluation.failed
        );
        merged += usize::from(touched.len() > 1);
        let (holder, consumed) = touched.split_first().unwrap();
        let made = solid_of(&evaluation, *holder);
        assert_near(made.volume(), cells.volume());
        let expected: Vec<_> = consumed.iter().map(|&body| (body, *holder)).collect();
        assert_eq!(evaluation.merged, expected, "{context}");
        let left: Vec<BodyId> = evaluation.bodies.iter().map(|made| made.body).collect();
        let kept: Vec<BodyId> = (ids.iter().copied())
            .filter(|id| !consumed.contains(id))
            .collect();
        assert_eq!(left, kept, "{context}");
        (ids.iter())
            .filter(|id| !touched.contains(id))
            .for_each(unchanged);
    }
    // The seeds hold both kinds.
    assert!(
        merged >= 12 && refused >= 2,
        "{merged} merged, {refused} refused"
    );
}

/// What a boolean whose result would touch itself fails with.
const TOUCHES_ITSELF: &str = "leaves no clean solid: the result would touch itself along an \
                              edge or at a point";

/// A join whose union with the body would touch itself fails saying so,
/// and leaves the body as it was: a box on a box's edge or corner, and a
/// lid flush on a pocket's rim (its bottom meets the body's top only
/// along the rim's edges, over the open pocket).
#[test]
fn a_join_whose_union_would_touch_itself_says_so() {
    // Blocks start or end on the sketch plane, z = 0.
    let body: Block = [0.0, 0.0, -10.0, 10.0, 10.0, 0.0];
    for (name, pocket, join) in [
        ("edge", None, [10.0, 10.0, -10.0, 20.0, 20.0, 0.0]),
        ("corner", None, [10.0, 10.0, 0.0, 20.0, 20.0, 10.0]),
        (
            "lid",
            Some([3.0, 3.0, -5.0, 7.0, 7.0, 0.0]),
            [3.0, 3.0, 0.0, 7.0, 7.0, 2.0],
        ),
    ] {
        let mut editor = Editor::new(Document::default());
        add_block(&mut editor, body, Operation::NewBody(BodyId::NEW));
        let id = editor.document().bodies()[0].id;
        let mut volume = block_volume(&body);
        if let Some(pocket) = pocket {
            add_block(&mut editor, pocket, Operation::Cut(Targets::default()));
            volume -= block_volume(&pocket);
        }
        let feature = add_block(&mut editor, join, Operation::Join(Targets::default()));
        let evaluation = evaluated(editor.document());
        assert_eq!(
            evaluation.touched.last(),
            Some(&(feature, vec![id])),
            "{name}"
        );
        let [
            crate::FeatureFailure {
                feature: failed,
                message: error,
                ..
            },
        ] = &evaluation.failed[..]
        else {
            panic!("{name}: {:?}", evaluation.failed);
        };
        assert_eq!(*failed, feature, "{name}");
        assert_eq!(
            *error,
            format!(
                "joining it to Body 1 {TOUCHES_ITSELF}, or come too close to itself; move it \
                 to overlap more or to clear it"
            ),
            "{name}"
        );
        let [made] = &evaluation.bodies[..] else {
            panic!("{name}: {} bodies", evaluation.bodies.len());
        };
        assert_near(made.solid.volume(), volume);
    }
}

/// Two blocks 10 mm on a side along y, `a` from y = -10 to -2 and `b`
/// from -30 to -20, and a bar 4 × 6 drawn on XZ joined `reach` along -y
/// from y = 0: the editor, the blocks' bodies and the join. At 15 it
/// reaches only `a`, at 25 both.
fn bar_along_y(reach: &str) -> (Editor, [BodyId; 2], FeatureId) {
    let mut editor = Editor::new(Document::default());
    let a = add_body(&mut editor, rectangle((0.0, -10.0), (10.0, -2.0)), "10");
    let b = add_body(&mut editor, rectangle((0.0, -30.0), (10.0, -20.0)), "10");
    let extent = Extent::OneSide(length(editor.document(), reach));
    let join = add_extrude_on(
        &mut editor,
        OriginPlane::XZ,
        rectangle((3.0, 2.0), (7.0, 8.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    (editor, [a, b], join)
}

/// The blocks of [`bar_along_y`].
const A: f64 = 10.0 * 8.0 * 10.0;
const B: f64 = 10.0 * 10.0 * 10.0;

/// The bar's cross-section.
const BAR: f64 = 4.0 * 6.0;

/// A join dragged out until it reaches a second body merges it, and
/// back apart: each answer is the document's as it stands, and going back
/// and forth, by edits or by undo and redo, finds every boolean again.
#[test]
fn a_join_reaching_a_second_body_merges_it_and_reaching_back_parts_them() {
    let (mut editor, [a, b], join) = bar_along_y("15");
    let mut cache = Cache::default();
    let apart = |evaluation: &Evaluation| {
        assert!(evaluation.failed.is_empty(), "{:?}", evaluation.failed);
        assert_eq!(evaluation.touched, [(join, vec![a])]);
        assert!(evaluation.merged.is_empty());
        // The bar is inside a for 8 of its 15.
        assert_near(solid_of(evaluation, a).volume(), A + BAR * 7.0);
        assert_near(solid_of(evaluation, b).volume(), B);
    };
    let together = |evaluation: &Evaluation| {
        assert_eq!(evaluation.touched, [(join, vec![a, b])]);
        assert_eq!(evaluation.merged, [(b, a)]);
        // 8 of its 25 in a, 5 in b.
        assert_near(only_body(evaluation).volume(), A + B + BAR * 12.0);
    };
    apart(&evaluate(editor.document(), &mut cache));
    let reach = |editor: &mut Editor, reach: &str| {
        let extent = Extent::OneSide(length(editor.document(), reach));
        set_extrude(editor, join, |extrude| extrude.extent = extent);
    };
    reach(&mut editor, "25");
    together(&evaluate(editor.document(), &mut cache));
    let worked = cache.counts().1;

    editor.undo();
    apart(&evaluate(editor.document(), &mut cache));
    editor.redo();
    together(&evaluate(editor.document(), &mut cache));
    reach(&mut editor, "15");
    apart(&evaluate(editor.document(), &mut cache));
    assert_eq!(cache.counts().1, worked, "nothing worked out again");
}

/// Deleting the body a join merged another into leaves the join in the
/// other alone, and deleting the one merged leaves it in the first alone.
#[test]
fn deleting_either_merged_body_leaves_the_join_in_the_other() {
    let (editor, [a, b], join) = bar_along_y("25");
    for (gone, stays, volume) in [(b, a, A + BAR * 17.0), (a, b, B + BAR * 20.0)] {
        let mut editor = Editor::new(editor.document().clone());
        let removal = editor
            .document()
            .removal(varde_document::Removable::Body(gone));
        assert_eq!(removal.bodies, [gone], "only the body and its maker go");
        editor.apply(Command::RemoveBody(gone)).unwrap();
        let evaluation = evaluated(editor.document());
        assert_eq!(evaluation.touched, [(join, vec![stays])]);
        assert!(evaluation.merged.is_empty());
        assert_eq!(evaluation.holder(gone), None);
        assert_near(only_body(&evaluation).volume(), volume);
    }
}

/// A cut after the merge that takes out the body merged away (as it did
/// before the join reached it) takes nothing out: that body's solid is
/// in the one holding it, which the cut works on.
#[test]
fn taking_a_merged_body_out_of_a_later_cut_takes_nothing_out() {
    let (mut editor, [a, b], _) = bar_along_y("25");
    let through = two_sides(editor.document(), "20", "20");
    let cut = add_extrude(
        &mut editor,
        rectangle((0.0, -26.0), (10.0, -24.0)),
        through,
        Operation::Cut(Targets {
            excluded: vec![b],
            held: None,
        }),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(evaluation.merged, [(b, a)]);
    assert_eq!(evaluation.touched[1], (cut, vec![a]));
    // A slot 2 wide across b, the bar's end inside it.
    assert_near(
        only_body(&evaluation).volume(),
        A + B + BAR * 12.0 - 10.0 * 2.0 * 10.0,
    );
}

/// A join whose tool joins the first body it touches but leaves the
/// second meeting that one along an edge it doesn't cover fails merging
/// them, showing where: the pinch, along the uncovered stretch of the
/// edge, and the faces named of either body, on that body.
#[test]
fn a_merge_that_fails_shows_where() {
    let mut editor = Editor::new(Document::default());
    add_block(
        &mut editor,
        [0.0, 0.0, 0.0, 10.0, 10.0, 10.0],
        Operation::NewBody(BodyId::NEW),
    );
    add_block(
        &mut editor,
        [10.0, 10.0, 0.0, 20.0, 20.0, 10.0],
        Operation::NewBody(BodyId::NEW),
    );
    let [a, b] = [0, 1].map(|i| editor.document().bodies()[i].id);
    // Inside the first, flush with the second's side up to z = 8.
    let join = add_block(
        &mut editor,
        [5.0, 5.0, 0.0, 10.0, 15.0, 8.0],
        Operation::Join(Targets::default()),
    );
    let crate::Response::Regenerated { failed, .. } =
        crate::handle(crate::tests::regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, join);
    assert!(
        failure.message.starts_with("merging Body 2 into Body 1"),
        "{}",
        failure.message
    );
    let geometry = failure.geometry.as_ref().expect("geometry");
    let points = geometry.points();
    assert!(!points.is_empty());
    for &[x, y, z] in points {
        assert!(
            (x - 10.0).abs() < 1e-3 && (y - 10.0).abs() < 1e-3,
            "{x} {y}"
        );
        assert!((7.999..=10.001).contains(&z), "{z}");
    }
    assert!(geometry.mesh().triangle_count() > 0);
    assert!(
        geometry
            .faces()
            .iter()
            .all(|&(body, _)| body == a || body == b)
    );
}
