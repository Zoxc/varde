//! An exact oracle for feet: the set `{s : ‖C(s) − q‖ ≤ r}` worked out
//! on the stored `f64`s as rationals, `F = ‖P − q·W‖² − r²·W²` in the
//! power basis, its distinct roots on each span isolated by Sturm
//! sequences, the set's components joined across knots (and through a
//! closed edge's vertex); each foot's ends checked to hold the
//! component's boundary point exactly.

use malachite::Rational as Q;

use super::*;

type P = Vec<Q>;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn zero() -> Q {
    Q::from(0u32)
}

fn trim(mut p: P) -> P {
    while p.last().is_some_and(|c| *c == 0u32) {
        p.pop();
    }
    p
}

fn add(a: &P, b: &P) -> P {
    let n = a.len().max(b.len());
    trim(
        (0..n)
            .map(|i| {
                a.get(i).cloned().unwrap_or_else(zero) + b.get(i).cloned().unwrap_or_else(zero)
            })
            .collect(),
    )
}

fn scale(a: &P, k: &Q) -> P {
    trim(a.iter().map(|c| c * k).collect())
}

fn mul(a: &P, b: &P) -> P {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![zero(); a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    trim(out)
}

fn eval_p(p: &P, t: &Q) -> Q {
    let mut v = zero();
    for c in p.iter().rev() {
        v = v * t + c;
    }
    v
}

fn deriv(p: &P) -> P {
    trim(
        p.iter()
            .enumerate()
            .skip(1)
            .map(|(i, c)| c * Q::from(i as u64))
            .collect(),
    )
}

/// The quotient and remainder of `a` by `b ≠ 0`.
fn divmod(a: &P, b: &P) -> (P, P) {
    let mut r = a.clone();
    let lead = b.last().expect("a non-zero divisor").clone();
    let mut quot = vec![zero(); a.len().saturating_sub(b.len()) + 1];
    while r.len() >= b.len() && !r.is_empty() {
        let k = r.last().expect("non-empty") / &lead;
        let shift = r.len() - b.len();
        for (i, c) in b.iter().enumerate() {
            r[i + shift] -= &k * c;
        }
        quot[shift] = k;
        r.pop();
        r = trim(r);
    }
    (trim(quot), r)
}

/// `a / b`, which divides exactly.
fn div(a: &P, b: &P) -> P {
    let (quot, r) = divmod(a, b);
    assert!(r.is_empty(), "inexact division");
    quot
}

/// The Bernstein basis polynomials of degree `n` in the power basis.
fn bernstein(n: usize) -> Vec<P> {
    let t: P = vec![zero(), Q::from(1u32)];
    let u: P = vec![Q::from(1u32), Q::from(-1i32)];
    (0..=n)
        .map(|j| {
            let mut p: P = vec![Q::from(binom(n, j))];
            for _ in 0..j {
                p = mul(&p, &t);
            }
            for _ in j..n {
                p = mul(&p, &u);
            }
            p
        })
        .collect()
}

fn binom(n: usize, k: usize) -> u64 {
    (0..k).fold(1u64, |acc, i| acc * (n - i) as u64 / (i as u64 + 1))
}

/// `F` of a span (affine control points with weights) in the power basis.
fn foot_f(pts: &[[f64; 4]], c: DVec3, r: f64) -> P {
    let b = bernstein(pts.len() - 1);
    let c = [q(c.x), q(c.y), q(c.z)];
    let mut sum: P = Vec::new();
    let mut w: P = Vec::new();
    for (j, p) in pts.iter().enumerate() {
        w = add(&w, &scale(&b[j], &q(p[3])));
    }
    for k in 0..3 {
        let mut x: P = Vec::new();
        for (j, p) in pts.iter().enumerate() {
            x = add(&x, &scale(&b[j], &(q(p[3]) * (q(p[k]) - &c[k]))));
        }
        sum = add(&sum, &mul(&x, &x));
    }
    let rr = q(r) * q(r);
    add(&sum, &scale(&mul(&w, &w), &-rr))
}

/// A Sturm sequence: the number of distinct roots of its first
/// polynomial on an interval from sign changes at its ends.
struct Sturm(Vec<P>);

impl Sturm {
    /// The sequence of `f ≠ 0`: `f`, `f'`, then negated remainders.
    fn of(f: &P) -> Sturm {
        let mut s = vec![f.clone(), deriv(f)];
        while s.last().is_some_and(|p| !p.is_empty()) {
            let n = s.len();
            let (_, r) = divmod(&s[n - 2], &s[n - 1]);
            s.push(scale(&r, &Q::from(-1i32)));
        }
        s.pop();
        Sturm(s)
    }

    /// The squarefree part of `f ≠ 0` and its sequence, and the gcd of
    /// `f` and `f'`, which vanishes exactly at `f`'s multiple roots.
    fn squarefree(f: &P) -> (P, Sturm, P) {
        let g = Sturm::of(f).0.pop().expect("f itself at least");
        let sq = div(f, &g);
        let s = Sturm::of(&sq);
        (sq, s, g)
    }

    fn var(&self, t: &Q) -> u32 {
        let mut last = None;
        let mut n = 0;
        for p in &self.0 {
            let v = eval_p(p, t);
            if v == 0u32 {
                continue;
            }
            let s = v > 0u32;
            if last.is_some_and(|l| l != s) {
                n += 1;
            }
            last = Some(s);
        }
        n
    }

    /// Distinct roots in `(a, b]`.
    fn count(&self, a: &Q, b: &Q) -> u32 {
        self.var(a) - self.var(b)
    }
}

/// A root of `F` in `[lo, hi]`, exact when the two are equal.
#[derive(Clone, Debug)]
struct Root {
    lo: Q,
    hi: Q,
}

impl Root {
    fn exact(&self) -> Option<&Q> {
        (self.lo == self.hi).then_some(&self.lo)
    }

    fn approx(&self) -> f64 {
        f64::rounding_from(&self.lo, Nearest).0
    }

    /// Halves the bracket (an inexact root lies in its open interior).
    fn refine(&mut self, s: &Sturm, f: &P) {
        let m: Q = (&self.lo + &self.hi) >> 1u32;
        if eval_p(f, &m) == 0u32 {
            self.lo = m.clone();
            self.hi = m;
        } else if s.count(&self.lo, &m) == 1 {
            self.hi = m;
        } else {
            self.lo = m;
        }
    }

    /// Whether the root lies in `[t0, t1]`.
    fn within(&self, s: &Sturm, f: &P, t0: f64, t1: f64) -> bool {
        let (t0, t1) = (q(t0), q(t1));
        let mut r = self.clone();
        loop {
            if let Some(x) = r.exact() {
                return t0 <= *x && *x <= t1;
            }
            // An inexact root lies in the open `(lo, hi)`.
            if r.hi <= t0 || r.lo >= t1 {
                return false;
            }
            if t0 <= r.lo && r.hi <= t1 {
                return true;
            }
            // Split at `t0` or `t1`, whichever is inside.
            let c = if r.lo < t0 { t0.clone() } else { t1.clone() };
            if eval_p(f, &c) == 0u32 {
                r.lo = c.clone();
                r.hi = c;
            } else if s.count(&r.lo, &c) == 1 {
                r.hi = c;
            } else {
                r.lo = c;
            }
        }
    }
}

/// The distinct roots of `f` on `[0, 1]`, sorted, brackets apart.
fn roots(f: &P, s: &Sturm) -> Vec<Root> {
    let (z, one) = (zero(), Q::from(1u32));
    let mut out = Vec::new();
    if eval_p(f, &z) == 0u32 {
        out.push(Root {
            lo: z.clone(),
            hi: z.clone(),
        });
    }
    let eps = Q::from(1u32) >> 80u32;
    let mut stack = vec![(z, one)];
    while let Some((a, b)) = stack.pop() {
        let n = s.count(&a, &b);
        if n == 0 {
            continue;
        }
        if n == 1 {
            let mut r = Root { lo: a, hi: b };
            if eval_p(f, &r.hi) == 0u32 {
                r.lo = r.hi.clone();
            }
            while r.exact().is_none() && &r.hi - &r.lo > eps {
                r.refine(s, f);
            }
            out.push(r);
            continue;
        }
        let m: Q = (&a + &b) >> 1u32;
        stack.push((m.clone(), b));
        stack.push((a, m));
    }
    out.sort_by(|x, y| x.lo.cmp(&y.lo));
    // Brackets strictly apart, and off 0 and 1 unless exact there.
    loop {
        let mut again = false;
        for k in 0..out.len() {
            let apart = k + 1 == out.len() || out[k].hi < out[k + 1].lo;
            let off = out[k].exact().is_some() || (out[k].lo > 0u32 && out[k].hi < 1u32);
            if !apart || !off {
                if out[k].exact().is_none() {
                    out[k].refine(s, f);
                    again = true;
                }
                if !apart && out[k + 1].exact().is_none() {
                    out[k + 1].refine(s, f);
                    again = true;
                }
            }
        }
        if !again {
            break;
        }
    }
    out
}

/// A boundary point of the set: an edge end, or a root on a span.
#[derive(Clone, Debug)]
enum Bound {
    EdgeStart,
    EdgeEnd,
    Root(usize, Root),
}

/// A component of the set, from `lo` to `hi` along the stored direction
/// (through a closed edge's vertex if it `wraps`).
#[derive(Clone, Debug)]
struct Comp {
    lo: Bound,
    hi: Bound,
    wraps: bool,
    /// Approximate global positions of the ends (span + t).
    at: [f64; 2],
}

/// A piece of a span in parameter order: an open gap between roots,
/// inside the set or not, or a root.
enum Atom {
    Gap(bool),
    Pt(Root),
}

/// The exact set on a chain: its components, and each span's squarefree
/// `F` with its Sturm sequence (none where `F ≡ 0`).
struct Exact {
    comps: Vec<Comp>,
    fs: Vec<(P, Option<Sturm>)>,
    /// Whether some span's `F` has a multiple root on `[0, 1]` or is zero.
    degenerate: bool,
}

/// The exact components of the set on the chain.
fn components(chain: &[&[[f64; 4]]], c: DVec3, r: f64, closed: bool) -> Exact {
    let n = chain.len();
    let mut fs = Vec::new();
    let mut atoms: Vec<(usize, Atom)> = Vec::new();
    let mut degenerate = false;
    for (i, s) in chain.iter().enumerate() {
        let f = foot_f(s, c, r);
        if f.is_empty() {
            degenerate = true;
            atoms.push((i, Atom::Gap(true)));
            fs.push((f, None));
            continue;
        }
        let (sq, st, g) = Sturm::squarefree(&f);
        let rs = roots(&sq, &st);
        // A root is multiple iff `g` vanishes there; the bracket holds no
        // other root of `f`, so none of `g`.
        let gs = Sturm::squarefree(&g).1;
        degenerate |= rs.iter().any(|rt| match rt.exact() {
            Some(x) => eval_p(&g, x) == 0u32,
            None => gs.count(&rt.lo, &rt.hi) > 0,
        });
        let gap = |a: &Q, b: &Q| -> Atom {
            let m: Q = (a + b) >> 1u32;
            Atom::Gap(eval_p(&f, &m) < 0u32)
        };
        let mut prev = zero();
        for rt in &rs {
            if rt.lo > prev {
                atoms.push((i, gap(&prev, &rt.lo)));
            }
            prev = rt.hi.clone();
            atoms.push((i, Atom::Pt(rt.clone())));
        }
        if prev < 1u32 {
            atoms.push((i, gap(&prev, &Q::from(1u32))));
        }
        fs.push((sq, Some(st)));
    }
    let mut comps: Vec<Comp> = Vec::new();
    let mut open: Option<(Bound, f64)> = None;
    let mut last: Option<(usize, Root)> = None;
    for (k, (i, atom)) in atoms.into_iter().enumerate() {
        let base = i as f64;
        match atom {
            Atom::Gap(true) => {
                if open.is_none() {
                    assert!(k == 0, "oracle: an inside gap without a start");
                    open = Some((Bound::EdgeStart, 0.0));
                }
            }
            Atom::Gap(false) => {
                if let Some((lo, at)) = open.take() {
                    let (j, rt) = last.clone().expect("a root before an outside gap");
                    comps.push(Comp {
                        lo,
                        at: [at, j as f64 + rt.approx()],
                        hi: Bound::Root(j, rt),
                        wraps: false,
                    });
                }
            }
            Atom::Pt(rt) => {
                if open.is_none() {
                    let start = i == 0 && rt.exact().is_some_and(|x| *x == 0u32);
                    let lo = if start {
                        Bound::EdgeStart
                    } else {
                        Bound::Root(i, rt.clone())
                    };
                    open = Some((lo, base + rt.approx()));
                }
                last = Some((i, rt));
            }
        }
    }
    if let Some((lo, at)) = open {
        comps.push(Comp {
            lo,
            hi: Bound::EdgeEnd,
            wraps: false,
            at: [at, n as f64],
        });
    }
    if closed
        && comps.len() >= 2
        && matches!(comps[0].lo, Bound::EdgeStart)
        && matches!(comps[comps.len() - 1].hi, Bound::EdgeEnd)
    {
        let end = comps.pop().unwrap();
        comps[0].lo = end.lo;
        comps[0].wraps = true;
        comps[0].at[0] = end.at[0];
    }
    Exact {
        comps,
        fs,
        degenerate,
    }
}

impl Exact {
    /// Whether the foot end (span, bracket, kind) is the bound.
    fn end_is(&self, e: &FootEnd, b: &Bound, n: usize, hi: bool) -> Result<(), String> {
        match b {
            Bound::EdgeStart | Bound::EdgeEnd => {
                let want = matches!(b, Bound::EdgeEnd);
                if e.kind != EndKind::Vertex || want != hi {
                    return Err(format!("{e:?} should be the edge end {b:?}"));
                }
                let (span, t) = if want { (n - 1, 1.0) } else { (0, 0.0) };
                if e.span as usize != span || e.t != [t, t] {
                    return Err(format!("{e:?} not at the edge end"));
                }
                Ok(())
            }
            Bound::Root(i, rt) => {
                if e.kind == EndKind::Vertex {
                    return Err(format!(
                        "{e:?} at a vertex, the set ends at a root {i} {rt:?}"
                    ));
                }
                let j = e.span as usize;
                let ok = if j == *i {
                    let (f, s) = &self.fs[*i];
                    rt.within(s.as_ref().expect("roots"), f, e.t[0], e.t[1])
                } else if j == *i + 1 {
                    rt.exact().is_some_and(|x| *x == 1u32) && e.t[0] == 0.0
                } else if j + 1 == *i {
                    rt.exact().is_some_and(|x| *x == 0u32) && e.t[1] == 1.0
                } else {
                    false
                };
                if !ok {
                    return Err(format!("{e:?} doesn't hold the root on span {i}: {rt:?}"));
                }
                if e.kind == EndKind::OnSphere {
                    let exact_end = rt.exact().is_some_and(|x| *x == 0u32 || *x == 1u32);
                    if !exact_end || e.t[0] != e.t[1] {
                        return Err(format!("{e:?} on the sphere, the root is {rt:?}"));
                    }
                }
                Ok(())
            }
        }
    }

    /// The feet against the exact components.
    fn check(&self, got: &Feet, n: usize) -> Result<(), String> {
        let mut c = 0;
        for f in &got.feet {
            if c >= self.comps.len() {
                return Err(format!("a foot past the components: {f:?}"));
            }
            let first = &self.comps[c];
            if f.wraps != first.wraps {
                return Err(format!("wraps {f:?} vs {first:?}"));
            }
            // A hull foot may take several components it can't separate.
            let reach = f.hi.span as f64 + f.hi.t[1];
            let mut k = c + 1;
            if f.hull && !f.wraps {
                while k < self.comps.len() && self.comps[k].at[0] <= reach + 1e-12 {
                    k += 1;
                }
            }
            if !f.hull && k != c + 1 {
                return Err("non-hull foot over several components".into());
            }
            let lastc = &self.comps[k - 1];
            self.end_is(&f.lo, &first.lo, n, false)
                .map_err(|e| format!("lo: {e}\n foot {f:?}\n comp {first:?}"))?;
            self.end_is(&f.hi, &lastc.hi, n, true)
                .map_err(|e| format!("hi: {e}\n foot {f:?}\n comp {lastc:?}"))?;
            c = k;
        }
        if c != self.comps.len() {
            return Err(format!(
                "{} components, feet cover {c}: {:?}",
                self.comps.len(),
                self.comps
            ));
        }
        if !self.degenerate && got.feet.iter().any(|f| f.hull) {
            return Err("a hull foot without a multiple root".into());
        }
        Ok(())
    }
}

/// Checks the feet of `q` on the chain against the exact set.
fn exact_check(chain: &[Span], c: DVec3, r: f64, closed: bool, what: &str) -> Feet {
    let curve = EdgeCurve::Spans {
        spans: chain,
        closed,
    };
    let got = feet(c, curve, r);
    let refs: Vec<&[[f64; 4]]> = chain.iter().map(|s| &s[..]).collect();
    let ex = components(&refs, c, r, closed);
    if let Err(e) = ex.check(&got, chain.len()) {
        panic!("{what}: {e}\n feet {got:?}\n q {c:?} r {r}\n chain {chain:?}");
    }
    got
}

/// The same on a straight edge.
fn exact_line(a: DVec3, b: DVec3, c: DVec3, r: f64, what: &str) -> Feet {
    let got = feet(c, EdgeCurve::Line([a, b]), r);
    let seg = [[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]];
    let ex = components(&[&seg[..]], c, r, false);
    if let Err(e) = ex.check(&got, 1) {
        panic!("{what}: {e}\n feet {got:?}\n q {c:?} r {r}\n line {a:?} {b:?}");
    }
    got
}

fn pick<T: Copy>(rng: &mut varde_testkit::rng::Rng, xs: &[T]) -> T {
    xs[(rng.next_u64() % xs.len() as u64) as usize]
}

/// Random conic chains, open and closed, some with weights 1/64 and 64.
#[test]
fn random_chains_against_the_oracle() {
    for seed in varde_testing::seeds(128, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut chain: Vec<Span> = Vec::new();
        for i in 0..n {
            let mut s = super::conic(&mut rng);
            if rng.next_u64().is_multiple_of(4) {
                for p in &mut s {
                    p[3] = pick(&mut rng, &[1.0 / 64.0, 64.0, 1.0, 0.5, 2.0]);
                }
            }
            if i > 0 {
                let p = chain[i - 1][2];
                s[0] = [p[0], p[1], p[2], s[0][3]];
            }
            chain.push(s);
        }
        let closed = n >= 2 && rng.next_u64().is_multiple_of(3);
        if closed {
            let p = chain[0][0];
            chain[n - 1][2] = [p[0], p[1], p[2], chain[n - 1][2][3]];
        }
        let q = rng.point(4.0);
        let r = rng.log_range(0.05, 6.0);
        exact_check(&chain, q, r, closed, &format!("seed {seed}"));
    }
}

/// Straight edges, up to `10⁹` out, radii from `10⁻⁶` to `10³`.
#[test]
fn straight_edges_against_the_oracle() {
    for seed in varde_testing::seeds(256, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let far = pick(&mut rng, &[1.0, 1e3, 1e6, 1e9]);
        let o = rng.point(far);
        let a = o + rng.point(4.0);
        let b = o + rng.point(4.0);
        let q = o + rng.point(4.0);
        let r = rng.log_range(1e-6, 1e3);
        exact_line(a, b, q, r, &format!("seed {seed}"));
    }
}

/// The cases of the tests above: the arc on, tangent to and about the
/// sphere, the V's knot on it, the square about its vertex and centre.
#[test]
fn known_cases_against_the_oracle() {
    let a = &[super::ARC];
    exact_check(a, DVec3::ZERO, 25.0, false, "on sphere");
    exact_check(a, DVec3::new(40.0, 30.0, 0.0), 25.0, false, "tangent out");
    exact_check(a, DVec3::new(-4.0, -3.0, 0.0), 30.0, false, "tangent in");
    exact_check(a, DVec3::new(-4.0, -3.0, 0.0), 29.999, false, "in-");
    exact_check(a, DVec3::new(20.0, 15.0, 0.0), 0.0, false, "r0");
    exact_check(a, DVec3::new(7.0, 24.0, 0.0), 0.0, false, "r0 vertex");
    let v = super::vee();
    exact_check(&v, DVec3::new(0.0, -1.0, 0.0), 1.0, false, "vee");
    let sq = super::square();
    for (c, r) in [
        (DVec3::ZERO, 1.0),
        (DVec3::ZERO, 4.5),
        (DVec3::new(2.0, 2.0, 0.0), 2.0),
        (DVec3::new(2.0, 2.0, 0.0), 2.5),
        (DVec3::new(2.0, 2.0, 0.0), 3.0),
        (DVec3::new(0.0, 1.0, 0.0), 1.2),
        (DVec3::new(-3.0, -4.0, 0.0), 5.0),
        (DVec3::new(0.0, 0.0, 3.0), 3.0),
        (DVec3::new(2.0, 2.0, 0.0), 8f64.sqrt()),
    ] {
        exact_check(&sq, c, r, true, &format!("square {c} {r}"));
        exact_check(&sq, c, r, false, &format!("open square {c} {r}"));
    }
}

/// The oracle refuses wrong feet.
#[test]
fn the_oracle_refuses_wrong_feet() {
    let v = super::vee();
    let refs = [&v[0][..], &v[1][..]];
    let ex = components(&refs, DVec3::new(0.0, -1.0, 0.0), 1.5, false);
    assert!(ex.check(&Feet::default(), 2).is_err());
    let got = feet(DVec3::new(0.0, -1.0, 0.0), spans(&v), 1.5);
    assert!(ex.check(&got, 2).is_ok());
    let mut bad = got.clone();
    bad.feet[0].lo.t = [0.9, 0.95];
    assert!(ex.check(&bad, 2).is_err());
}

/// Every check on `chain`, stored the other way too, and (polynomial
/// spans) cut at eighths: each against the exact set, and the feet in
/// geometric order the same both ways round.
fn check_all(
    rng: &mut varde_testkit::rng::Rng,
    chain: &[Span],
    c: DVec3,
    r: f64,
    closed: bool,
    what: &str,
) {
    let got = exact_check(chain, c, r, closed, what);
    let rev: Vec<Span> = chain.iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
    let back = exact_check(&rev, c, r, closed, &format!("{what} reversed"));
    assert_eq!(
        got.feet.len(),
        back.feet.len(),
        "{what}: stored the other way"
    );
    if !closed {
        // Global position of a foot's start, from the original's start.
        let n = chain.len() as f64;
        let a: Vec<f64> = got
            .in_order(Along::Stored)
            .map(|k| {
                let f = got.get(k).unwrap();
                f.lo.span as f64 + f.lo.t[0]
            })
            .collect();
        let b: Vec<f64> = back
            .in_order(Along::Reversed)
            .map(|k| {
                let f = back.get(k).unwrap();
                n - (f.hi.span as f64 + f.hi.t[1])
            })
            .collect();
        for (x, y) in a.iter().zip(&b) {
            assert!(
                (x - y).abs() < 1e-6 || got.feet.iter().any(|f| f.hull),
                "{what}: order {a:?} {b:?}"
            );
        }
    }
    if chain.iter().all(|s| s.iter().all(|p| p[3] == 1.0)) {
        let mut cut: Vec<Span> = Vec::new();
        for s in chain {
            let mut at: Vec<f64> = (0..rng.next_u64() % 3)
                .map(|_| (1 + rng.next_u64() % 7) as f64 / 8.0)
                .collect();
            at.sort_by(f64::total_cmp);
            at.dedup();
            let b: Vec<f64> = std::iter::once(0.0)
                .chain(at)
                .chain(std::iter::once(1.0))
                .collect();
            for w in b.windows(2) {
                cut.push(super::piece(s, w[0], w[1]));
            }
        }
        // Exact only for small coordinates.
        let small = chain.iter().flatten().all(|p| {
            p[..3]
                .iter()
                .all(|x| x.abs() < 1e6 && (x * 64.0).fract() == 0.0)
        });
        if small {
            let cf = exact_check(&cut, c, r, closed, &format!("{what} cut"));
            assert_eq!(got.feet.len(), cf.feet.len(), "{what}: cut");
        }
    }
}

const QUADS: [[f64; 4]; 10] = [
    [1.0, 2.0, 2.0, 3.0],
    [2.0, 3.0, 6.0, 7.0],
    [1.0, 4.0, 8.0, 9.0],
    [4.0, 4.0, 7.0, 9.0],
    [2.0, 6.0, 9.0, 11.0],
    [6.0, 6.0, 7.0, 11.0],
    [3.0, 4.0, 12.0, 13.0],
    [0.0, 3.0, 4.0, 5.0],
    [0.0, 0.0, 1.0, 1.0],
    [0.0, 5.0, 12.0, 13.0],
];

/// A random integer vector of length a whole number, scaled by `s`.
fn pyth(rng: &mut varde_testkit::rng::Rng, s: f64) -> (DVec3, f64) {
    let v = pick(rng, &QUADS);
    let mut c = [v[0], v[1], v[2]];
    for i in (1..3).rev() {
        c.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
    }
    for x in &mut c {
        if rng.next_u64().is_multiple_of(2) {
            *x = -*x;
        }
    }
    (DVec3::from_array(c) * s, v[3] * s)
}

fn int(rng: &mut varde_testkit::rng::Rng, n: u64) -> f64 {
    (rng.next_u64() % (2 * n + 1)) as f64 - n as f64
}

/// A chain of spans with integer points (weights 1 or random powers of
/// two), possibly closed, some degenerate (middle point on the chord).
fn int_chain(rng: &mut varde_testkit::rng::Rng, closed: bool) -> Vec<Span> {
    let n = if closed {
        2 + rng.next_u64() % 2
    } else {
        1 + rng.next_u64() % 3
    } as usize;
    let weighted = rng.next_u64().is_multiple_of(2);
    let mut chain: Vec<Span> = Vec::new();
    for i in 0..n {
        let mut s: Span = std::array::from_fn(|_| {
            let w = if weighted {
                pick(rng, &[1.0 / 64.0, 0.25, 1.0, 4.0, 64.0])
            } else {
                1.0
            };
            [int(rng, 8), int(rng, 8), int(rng, 3), w]
        });
        if i > 0 {
            let p = chain[i - 1][2];
            s[0] = [p[0], p[1], p[2], s[0][3]];
        }
        if rng.next_u64().is_multiple_of(4) {
            // Middle point on the chord's line.
            let l = pick(rng, &[-1.0, 0.0, 0.5, 1.0, 2.0]);
            for k in 0..3 {
                s[1][k] = s[0][k] + l * (s[2][k] - s[0][k]);
            }
        }
        chain.push(s);
    }
    if closed {
        let p = chain[0][0];
        chain[n - 1][2] = [p[0], p[1], p[2], chain[n - 1][2][3]];
    }
    chain
}

/// Spheres through a vertex or knot exactly.
#[test]
fn spheres_through_ends_and_knots() {
    for seed in varde_testing::seeds(64, 1024) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let closed = rng.next_u64().is_multiple_of(3);
        let chain = int_chain(&mut rng, closed);
        let ends: Vec<[f64; 4]> = chain
            .iter()
            .map(|s| s[0])
            .chain(std::iter::once(chain[chain.len() - 1][2]))
            .collect();
        let k = pick(&mut rng, &ends);
        let s = pick(&mut rng, &[0.125, 0.5, 1.0, 2.0, 0.0]);
        let (v, r) = pyth(&mut rng, s);
        let c = DVec3::new(k[0], k[1], k[2]) + v;
        check_all(&mut rng, &chain, c, r, closed, &format!("seed {seed}"));
    }
}

/// Spheres tangent to a polynomial span at an eighth, from either side,
/// or at a span's end along its end tangent.
#[test]
fn spheres_tangent_to_spans() {
    let mut hulls = 0;
    for seed in varde_testing::seeds(64, 1024) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let closed = rng.next_u64().is_multiple_of(4);
        let chain = int_chain(&mut rng, closed);
        let i = (rng.next_u64() % chain.len() as u64) as usize;
        let s = chain[i];
        let poly = s.iter().all(|p| p[3] == 1.0);
        let t = if poly {
            (rng.next_u64() % 9) as f64 / 8.0
        } else {
            pick(&mut rng, &[0.0, 1.0])
        };
        let (point, tan) = if poly {
            let p = super::eval(&s, t);
            let d = |a: [f64; 4], b: [f64; 4]| DVec3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
            (p, d(s[0], s[1]) * (1.0 - t) + d(s[1], s[2]) * t)
        } else if t == 0.0 {
            (
                DVec3::new(s[0][0], s[0][1], s[0][2]),
                DVec3::new(s[1][0] - s[0][0], s[1][1] - s[0][1], s[1][2] - s[0][2]),
            )
        } else {
            (
                DVec3::new(s[2][0], s[2][1], s[2][2]),
                DVec3::new(s[2][0] - s[1][0], s[2][1] - s[1][1], s[2][2] - s[1][2]),
            )
        };
        // Integer normals of whole length.
        let mut normals = Vec::new();
        for x in -12i32..=12 {
            for y in -12i32..=12 {
                for z in -12i32..=12 {
                    let n = DVec3::new(x as f64, y as f64, z as f64);
                    let l2 = n.length_squared();
                    let l = l2.sqrt();
                    if l2 > 0.0 && l.fract() == 0.0 && n.dot(tan) == 0.0 {
                        normals.push((n, l));
                    }
                }
            }
        }
        if normals.is_empty() {
            continue;
        }
        let (n, l) = normals[(rng.next_u64() % normals.len() as u64) as usize];
        let k = pick(&mut rng, &[0.125, 0.25, 0.5, 1.0, 2.0]);
        let c = point + n * k;
        let r = match rng.next_u64() % 4 {
            0 => (l * k).next_up(),
            1 => (l * k).next_down(),
            _ => l * k,
        };
        let what = format!("seed {seed} span {i} t {t}");
        check_all(&mut rng, &chain, c, r, closed, &what);
        hulls += usize::from(
            feet(
                c,
                EdgeCurve::Spans {
                    spans: &chain,
                    closed,
                },
                r,
            )
            .feet
            .iter()
            .any(|f| f.hull),
        );
    }
    if varde_testing::replay_seed().is_none() {
        assert!(hulls > 0, "no tangency reached the hull rule");
    }
}

/// The rational point of the unit circle at stereographic parameter `t`,
/// as rationals.
fn circ(t: &Q) -> [Q; 2] {
    let one = Q::from(1u32);
    let d = &one + t * t;
    [(&one - t * t) / &d, (Q::from(2u32) * t) / d]
}

/// A chain of exact circle arcs (radius `R`, centre 0, in the plane z =
/// 0) through the points at stereographic parameters `ts` (increasing),
/// closed if asked (back to the first), scaled to integers; then mapped
/// by `m` (an integer matrix, orthogonal up to the factor `k`) and moved
/// by `o`. Returns the chain, the radius and the points of the circle at
/// the parameters `extra`, mapped the same way; `None` where a value
/// isn't a float.
fn circle_chain(
    ts: &[Q],
    closed: bool,
    m: [[i64; 3]; 3],
    k: i64,
    o: [i64; 3],
    extra: &[Q],
) -> Option<(Vec<Span>, f64, Vec<DVec3>)> {
    let pts: Vec<[Q; 2]> = ts.iter().map(circ).collect();
    // Each arc: its start, the tangents' meeting point, its end, weights.
    let mut arcs: Vec<([[Q; 2]; 3], [Q; 3])> = Vec::new();
    let n = pts.len();
    let m_arcs = if closed { n } else { n - 1 };
    for i in 0..m_arcs {
        let (a, b) = (&pts[i], &pts[(i + 1) % n]);
        let dot = &a[0] * &b[0] + &a[1] * &b[1];
        let one = Q::from(1u32);
        if dot <= Q::from(-1i32) + (Q::from(1u32) >> 4u32) {
            return None; // too near a half turn
        }
        // The side of b from a: arcs run counter-clockwise, under 180°.
        let cross = &a[0] * &b[1] - &a[1] * &b[0];
        if cross <= 0u32 {
            return None;
        }
        let den = &one + &dot;
        let mid = [(&a[0] + &b[0]) / &den, (&a[1] + &b[1]) / &den];
        // Weights (1, 1, 2/(1 + a·b)) up to a common factor.
        let w = [den.clone(), den.clone(), Q::from(2u32)];
        arcs.push(([a.clone(), mid, b.clone()], w));
    }
    // Clear denominators: coordinates by L, weights per span by their own.
    let mut l = malachite::Natural::from(1u32);
    use malachite::base::num::arithmetic::traits::Lcm;
    for (pts, _) in &arcs {
        for x in pts.iter().flatten() {
            l = l.lcm(x.denominator_ref());
        }
    }
    let extra: Vec<[Q; 2]> = extra.iter().map(circ).collect();
    for p in &extra {
        for x in p {
            l = l.lcm(x.denominator_ref());
        }
    }
    let lq = Q::from(l);
    let to_f = |x: &Q| -> Option<f64> {
        let f = f64::rounding_from(x, Nearest).0;
        (Q::try_from(f).ok().as_ref() == Some(x) && f.abs() < 1e9).then_some(f)
    };
    let mut chain = Vec::new();
    for (pts, w) in &arcs {
        let mut wl = malachite::Natural::from(1u32);
        for x in w {
            wl = wl.lcm(x.denominator_ref());
        }
        let wq = Q::from(wl);
        let mut span: Span = [[0.0; 4]; 3];
        for (j, p) in pts.iter().enumerate() {
            let v = [&p[0] * &lq, &p[1] * &lq, Q::from(0u32)];
            for row in 0..3 {
                let x: Q = (0..3).map(|c| Q::from(m[row][c]) * &v[c]).sum::<Q>() + Q::from(o[row]);
                span[j][row] = to_f(&x)?;
            }
            span[j][3] = to_f(&(&w[j] * &wq))?;
        }
        chain.push(span);
    }
    let mut out = Vec::new();
    for p in &extra {
        let v = [&p[0] * &lq, &p[1] * &lq, Q::from(0u32)];
        let mut c = [0.0; 3];
        for row in 0..3 {
            let x: Q = (0..3).map(|c| Q::from(m[row][c]) * &v[c]).sum::<Q>() + Q::from(o[row]);
            c[row] = to_f(&x)?;
        }
        out.push(DVec3::from_array(c));
    }
    let r = to_f(&(lq * Q::from(k)))?;
    Some((chain, r, out))
}

/// Curves on the sphere: chains of exact circle arcs, open and closed, in
/// tilted planes, the point at the centre or on the axis.
#[test]
fn circles_on_and_tangent_to_the_sphere() {
    let rots: [([[i64; 3]; 3], i64); 3] = [
        ([[1, 0, 0], [0, 1, 0], [0, 0, 1]], 1),
        ([[-1, 2, 2], [2, -1, 2], [2, 2, -1]], 3),
        ([[2, -6, 3], [3, 2, 6], [6, 3, -2]], 7),
    ];
    let mut done = 0;
    for seed in varde_testing::seeds(128, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let closed = rng.next_u64().is_multiple_of(2);
        let n = 2 + rng.next_u64() % 4;
        // Stereographic parameters: increasing, from −∞ side to +∞; for a
        // closed chain they go round once.
        let mut ts: Vec<Q> = (0..n)
            .map(|_| Q::from(int(&mut rng, 9) as i64) / Q::from(1 + rng.next_u64() % 3))
            .collect();
        ts.sort();
        ts.dedup();
        if ts.len() < 2 {
            continue;
        }
        let (m, k) = pick(&mut rng, &rots);
        let o = [
            int(&mut rng, 5) as i64,
            int(&mut rng, 5) as i64,
            int(&mut rng, 5) as i64,
        ];
        // Closed: the arc from the last back to the first passes t = ∞.
        // A point of the circle on the chain's first arc, or a knot.
        let tp = if rng.next_u64().is_multiple_of(2) {
            ts[(rng.next_u64() % ts.len() as u64) as usize].clone()
        } else {
            (&ts[0] + &ts[1]) / Q::from(2u32)
        };
        let Some((chain, r, extra)) = circle_chain(&ts, closed, m, k, o, &[tp]) else {
            continue;
        };
        let centre = DVec3::new(o[0] as f64, o[1] as f64, o[2] as f64);
        // The axis `m·(0, 0, 1)`, of length `k`: `h·axis/k` exact where the
        // radius is a multiple of `k`.
        let axis = DVec3::new(m[0][2] as f64, m[1][2] as f64, m[2][2] as f64);
        let kf = k as f64;
        let (c, rr) = match rng.next_u64() % 5 {
            0 => (centre, r),
            // On the axis: h = 3/4·r gives √(r² + h²) = 5/4·r.
            1 => (centre + axis * (0.75 * (r / kf)), 1.25 * r),
            2 => (
                centre,
                r * (1.0 + pick(&mut rng, &[1e-9, -1e-9, 1e-3, -1e-3])),
            ),
            // Tangent at the point, from outside (μ > 0) or inside.
            _ => {
                let mu = pick(&mut rng, &[0.5, 0.25, 1.5, -0.5, -1.0, 2.0]);
                let p = extra[0];
                let c = centre + (p - centre) * mu;
                let rr = (1.0 - mu).abs() * r;
                if rng.next_u64().is_multiple_of(2) {
                    (c, rr)
                } else {
                    (c + axis * (0.75 * (rr / kf)), 1.25 * rr)
                }
            }
        };
        let what = format!("seed {seed} ts {ts:?} closed {closed}");
        check_all(&mut rng, &chain, c, rr, closed, &what);
        done += 1;
    }
    if varde_testing::replay_seed().is_none() {
        assert!(done > 8, "{done} circle chains");
    }
}

/// Far out, tiny and huge radii.
#[test]
fn far_out_with_tiny_and_huge_radii() {
    for seed in varde_testing::seeds(128, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let far = pick(&mut rng, &[1e3, 1e6, 1e8, 4e9]);
        let o = (rng.point(far) * 4.0).round() / 4.0;
        let ext = pick(&mut rng, &[1e-6, 1e-3, 1.0, 1e3]);
        let mut chain: Vec<Span> = Vec::new();
        let n = 1 + (rng.next_u64() % 2) as usize;
        for i in 0..n {
            let mut s: Span = std::array::from_fn(|_| {
                let p = o + rng.point(ext);
                [p.x, p.y, p.z, pick(&mut rng, &[1.0, 1.0 / 64.0, 64.0, 3.0])]
            });
            if i > 0 {
                let p = chain[i - 1][2];
                s[0] = [p[0], p[1], p[2], s[0][3]];
            }
            chain.push(s);
        }
        let c = o + rng.point(ext * 2.0);
        let r = pick(
            &mut rng,
            &[5e-324, 1e-300, 1e-12, ext * 0.5, ext, 1e12, 1e300, f64::MAX],
        );
        let what = format!("seed {seed}");
        exact_check(&chain, c, r, false, &what);
        let a = DVec3::new(chain[0][0][0], chain[0][0][1], chain[0][0][2]);
        let b = DVec3::new(chain[0][2][0], chain[0][2][1], chain[0][2][2]);
        exact_line(a, b, c, r, &what);
    }
}

/// Degenerate spans: a point, a span folded back on itself, a zero-length
/// span inside a chain.
#[test]
fn degenerate_spans() {
    let p = [1.0, 2.0, 2.0];
    let pt = |w: [f64; 3]| -> Span {
        [
            [p[0], p[1], p[2], w[0]],
            [p[0], p[1], p[2], w[1]],
            [p[0], p[1], p[2], w[2]],
        ]
    };
    for w in [[1.0; 3], [1.0 / 64.0, 64.0, 1.0], [64.0, 1.0, 1.0 / 64.0]] {
        for r in [0.0, 2.0, 3.0, 4.0] {
            exact_check(&[pt(w)], DVec3::ZERO, r, false, &format!("point {w:?} {r}"));
        }
        // Folded: out to (4, 0, 0) and back.
        let fold: Span = [
            [0.0, 0.0, 0.0, w[0]],
            [8.0, 0.0, 0.0, w[1]],
            [0.0, 0.0, 0.0, w[2]],
        ];
        for (c, r) in [
            (DVec3::ZERO, 1.0),
            (DVec3::new(2.0, 0.0, 0.0), 1.0),
            (DVec3::new(0.0, 3.0, 0.0), 5.0),
            (DVec3::new(-1.0, 0.0, 0.0), 1.0),
            (DVec3::new(9.0, 0.0, 0.0), 1.0),
            (DVec3::new(2.0, 0.0, 0.0), 0.0),
        ] {
            exact_check(&[fold], c, r, false, &format!("fold {w:?} {c} {r}"));
        }
        // A zero-length span between two straight ones.
        let chain = [
            super::straight([-4.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            pt(w).map(|_| [0.0, 0.0, 0.0, 1.0]),
            super::straight([0.0, 0.0, 0.0], [4.0, 0.0, 0.0]),
        ];
        for (c, r) in [
            (DVec3::ZERO, 1.0),
            (DVec3::new(0.0, 3.0, 0.0), 3.0),
            (DVec3::new(0.0, 3.0, 4.0), 5.0),
            (DVec3::new(0.0, 3.0, 0.0), 5.0),
            (DVec3::new(-3.0, 4.0, 0.0), 5.0),
        ] {
            exact_check(&chain, c, r, false, &format!("zero-length {c} {r}"));
        }
    }
}

/// Extreme weights and scales.
#[test]
fn extreme_weights_and_scales() {
    for seed in varde_testing::seeds(64, 1024) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let scale = pick(&mut rng, &[1e-100, 1e-20, 1.0, 1e6]);
        let mut chain: Vec<Span> = Vec::new();
        for i in 0..2 {
            let mut s: Span = std::array::from_fn(|_| {
                let p = rng.point(4.0) * scale;
                [
                    p.x,
                    p.y,
                    p.z,
                    pick(&mut rng, &[1.0 / 64.0, 1.0, 64.0, 1e-6, 1e6]),
                ]
            });
            if i > 0 {
                let p = chain[i - 1][2];
                s[0] = [p[0], p[1], p[2], s[0][3]];
            }
            chain.push(s);
        }
        let c = rng.point(4.0) * scale;
        let r = rng.log_range(0.05, 6.0) * scale;
        exact_check(&chain, c, r, false, &format!("seed {seed}"));
    }
}

/// Osculating spheres of the parabola `y = x²`: contact of order three at
/// `x = 3/8` (the curve crosses the sphere there, a triple root) and of
/// order four at its vertex (touching from outside); off the plane too,
/// a radius an ulp either side, and cut at the contact point.
#[test]
fn osculating_spheres() {
    let par: Span = [
        [-1.0, 1.0, 0.0, 1.0],
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0],
    ];
    let mut rng = varde_testkit::rng::Rng::new(1);
    for (c, r) in [
        (DVec3::new(-27.0 / 128.0, 59.0 / 64.0, 0.0), 125.0 / 128.0),
        (DVec3::new(0.0, 0.5, 0.0), 0.5),
    ] {
        for (c, r) in [(c, r), (c + DVec3::new(0.0, 0.0, 0.75 * r), 1.25 * r)] {
            for (c, r) in [(c, r), (c, r.next_up()), (c, r.next_down())] {
                check_all(
                    &mut rng,
                    &[par],
                    c,
                    r,
                    false,
                    &format!("osculating {c} {r}"),
                );
                let cut = [
                    super::piece(&par, 0.0, 11.0 / 16.0),
                    super::piece(&par, 11.0 / 16.0, 1.0),
                ];
                check_all(
                    &mut rng,
                    &cut,
                    c,
                    r,
                    false,
                    &format!("osculating cut {c} {r}"),
                );
            }
        }
    }
}
