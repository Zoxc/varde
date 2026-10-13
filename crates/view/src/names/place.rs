//! Where labels go: points along what they name to try, the bodies'
//! outline on screen, and each label's free place near what it names.

use glam::{DVec2, DVec3};
use iced::{Point, Rectangle, Size};

use super::{GAP, LABEL_HEIGHT, MARK_ROOM, PAD, TRIES, Wanted};
use crate::status::STATUS_BAR_ROOM;

/// The direction of the segment of the polyline `points` nearest `p`, by
/// its middle; zero if it has none.
pub(super) fn tangent_at(points: &[DVec3], p: DVec3) -> DVec3 {
    (points.windows(2))
        .map(|w| (w[1] - w[0], (w[0] + w[1]) / 2.0))
        .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))
        .map_or(DVec3::ZERO, |(d, _)| d.normalize_or_zero())
}

/// Points along the polyline `points` to try a label at, the one
/// halfway first, then out towards its ends, [`TRIES`] at most.
pub(super) fn out_from_middle(points: &[glam::DVec3]) -> impl Iterator<Item = glam::DVec3> + '_ {
    let half = (TRIES / 2) as u32;
    let length: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    (0..=half)
        .flat_map(move |k| {
            let off = f64::from(k) / f64::from(half) / 2.0;
            [0.5 - off, 0.5 + off]
                .into_iter()
                .take(if k == 0 { 1 } else { 2 })
        })
        .filter_map(move |t| at_length(points, length * t))
}

/// Of `sorted`, the `first` few, then the rest evenly spaced, `most` in
/// all.
pub(super) fn spread<T: Copy>(
    sorted: &[T],
    first: usize,
    most: usize,
) -> impl Iterator<Item = T> + '_ {
    let (head, rest) = sorted.split_at(first.min(sorted.len()));
    let step = rest
        .len()
        .div_ceil(most.saturating_sub(first).max(1))
        .max(1);
    head.iter().chain(rest.iter().step_by(step)).copied()
}

/// The point `left` along the polyline `points`.
fn at_length(points: &[glam::DVec3], mut left: f64) -> Option<glam::DVec3> {
    for w in points.windows(2) {
        let d = w[0].distance(w[1]);
        if d >= left && d > 0.0 {
            return Some(w[0].lerp(w[1], left / d));
        }
        left -= d;
    }
    points.last().copied()
}

/// The point of `rect`'s border nearest `p`, or its nearest side's
/// middle height where `p` is beside it.
pub(super) fn nearest_on(rect: Rectangle, p: Point) -> Point {
    let x = p.x.clamp(rect.x, rect.x + rect.width);
    let y = p.y.clamp(rect.y, rect.y + rect.height);
    if p.x < rect.x || p.x > rect.x + rect.width {
        Point::new(x, rect.center_y())
    } else {
        Point::new(x, y)
    }
}

/// The convex hull of `points`, anticlockwise on screen (y down), by the
/// monotone chain.
pub(super) fn hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut pts: Vec<_> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Whether `p` is in `hull`, as [`hull`] gives it, or on its border.
fn inside_hull(hull: &[DVec2], p: DVec2) -> bool {
    hull.len() >= 3
        && (0..hull.len()).all(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            (b - a).perp_dot(p - a) >= 0.0
        })
}

/// Whether `rect` and the hull overlap: a sample of the rectangle's
/// points in the hull, or a hull point in the rectangle.
fn meets_hull(hull: &[DVec2], rect: Rectangle) -> bool {
    let (x0, y0) = (f64::from(rect.x), f64::from(rect.y));
    let (x1, y1) = (x0 + f64::from(rect.width), y0 + f64::from(rect.height));
    let samples = (0..=4).flat_map(|i| {
        let x = x0 + (x1 - x0) * f64::from(i) / 4.0;
        [
            DVec2::new(x, y0),
            DVec2::new(x, (y0 + y1) / 2.0),
            DVec2::new(x, y1),
        ]
    });
    samples.into_iter().any(|p| inside_hull(hull, p))
        || hull
            .iter()
            .any(|p| p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1)
}

/// Where each label goes: the free place nearest what it names, apart
/// from the labels placed before it, in the viewport, clear of the
/// status bar, and outside the bodies' outline `hull` unless there's
/// no room there. `None` for a label with no room at all.
pub(super) fn place(
    wanted: &[Wanted],
    widths: &[f32],
    hull: &[DVec2],
    size: Size,
) -> Vec<Option<Rectangle>> {
    let room = Rectangle::new(
        Point::ORIGIN,
        Size::new(size.width, (size.height - STATUS_BAR_ROOM).max(0.0)),
    );
    let centre = if hull.is_empty() {
        DVec2::new(f64::from(size.width), f64::from(size.height)) / 2.0
    } else {
        hull.iter().sum::<DVec2>() / hull.len() as f64
    };

    // The outermost first: they have the shortest way out, and leave the
    // places nearer the model to those behind them.
    let mut order: Vec<usize> = (0..wanted.len()).collect();
    order.sort_by(|&a, &b| {
        let d = |i: usize| wanted[i].anchor.distance_squared(centre);
        d(b).total_cmp(&d(a))
    });

    let mut placed: Vec<Rectangle> = Vec::new();
    let mut out = vec![None; wanted.len()];
    let fits = |rect: &Rectangle, placed: &[Rectangle]| {
        rect.x >= room.x
            && rect.y >= room.y
            && rect.x + rect.width <= room.width
            && rect.y + rect.height <= room.height
            && !placed.iter().any(|o| o.expand(GAP).intersects(rect))
    };
    for i in order {
        let want = &wanted[i];
        let width = widths[i] + 2.0 * PAD + MARK_ROOM;
        let label = Size::new(width, LABEL_HEIGHT);
        let anchor = Point::new(want.anchor.x as f32, want.anchor.y as f32);
        let at = |c: Point| {
            Rectangle::new(
                Point::new(c.x - label.width / 2.0, c.y - label.height / 2.0),
                label,
            )
        };
        let allowed =
            |rect: &Rectangle, outside: bool| !outside || !meets_hull(hull, rect.expand(GAP));

        // Rings of places round the anchor, nearest first, the first free
        // one taken: outside the model if it has room, else anywhere.
        'passes: for outside in [true, false] {
            let mut r = 0.0f32;
            let most = size.width.max(size.height);
            while r <= most {
                let steps = if r == 0.0 {
                    1
                } else {
                    ((r * std::f32::consts::TAU / 8.0) as usize).clamp(8, 96)
                };
                let mut best: Option<(f32, Rectangle)> = None;
                for s in 0..steps {
                    let angle = s as f32 / steps as f32 * std::f32::consts::TAU;
                    // The label's nearest side `r` from the anchor.
                    let (dx, dy) = (angle.cos(), angle.sin());
                    let c = Point::new(
                        anchor.x + dx * (r + label.width / 2.0 * dx.abs().min(1.0)),
                        anchor.y + dy * (r + label.height / 2.0 * dy.abs().min(1.0)),
                    );
                    let rect = at(c);
                    if fits(&rect, &placed) && allowed(&rect, outside) {
                        let length = anchor.distance(nearest_on(rect, anchor));
                        if best.is_none_or(|(l, _)| length < l) {
                            best = Some((length, rect));
                        }
                    }
                }
                if let Some((_, rect)) = best {
                    placed.push(rect);
                    out[i] = Some(rect);
                    break 'passes;
                }
                r += if r < 40.0 { 4.0 } else { 10.0 };
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_drops_the_points_inside() {
        let square = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)].map(DVec2::from);
        let mut points = square.to_vec();
        points.extend([DVec2::new(2.0, 2.0), DVec2::new(1.0, 3.0), DVec2::NAN]);
        let mut found = hull(&points);
        assert_eq!(found.len(), 4);
        found.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        let mut want = square.to_vec();
        want.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        assert_eq!(found, want);
        assert!(inside_hull(&hull(&points), DVec2::new(1.0, 1.0)));
        assert!(!inside_hull(&hull(&points), DVec2::new(5.0, 1.0)));
    }

    #[test]
    fn spread_takes_the_first_then_a_sample() {
        let sorted: Vec<u32> = (0..100).collect();
        let tried: Vec<u32> = spread(&sorted, 8, TRIES).collect();
        assert!(tried.len() <= TRIES);
        assert_eq!(&tried[..8], &sorted[..8]);
    }
}
