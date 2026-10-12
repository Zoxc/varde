//! Calls clippy.toml bans, one per line marked `// banned`, so tools/ci.sh
//! can check each is refused: it runs clippy on this file with
//! `--cfg ban_probe` and expects a disallowed-method or -type diagnostic on
//! exactly the marked lines. Empty in every other build.

#![cfg(ban_probe)]
#![allow(deprecated, unused_imports, reason = "only the bans matter here")]

use rayon::prelude::*;
use spade::{ConstrainedDelaunayTriangulation, DelaunayTriangulation, Point2, Triangulation};

#[test]
fn banned() {
    let x = 0.5f64;
    let _ = x.sin(); // banned
    let _ = x.atan2(1.0); // banned
    let _ = glam::DVec2::from_angle(x); // banned
    let _ = glam::DQuat::from_rotation_z(x); // banned
    let _ = glam::DMat4::from_rotation_x(x); // banned
    let _ = glam::DVec4::ONE.exp2(); // banned
    let _ = 0.5f32.cos(); // banned
    let _ = x.mul_add(2.0, 1.0); // banned
    let _ = x.min(0.0); // banned
    let _ = f64::max(x, -0.0); // banned
    let _ = glam::DVec3::ONE.min(glam::DVec3::ZERO); // banned
    let _ = glam::DVec3::ONE.clamp(glam::DVec3::ZERO, glam::DVec3::ONE); // banned
    let _ = [1.0f64, 2.0].par_iter().sum::<f64>(); // banned
    let _ = [1.0f64].par_iter().copied().reduce(|| 0.0, |a, b| a + b); // banned
    let _ = [1.0f64].par_iter().fold(|| 0.0, |a, b| a + b); // banned
    let _ = [1.0f64].par_iter().min_by(|a, b| a.total_cmp(b)); // banned
    let _ = [1.0f64].par_iter().find_any(|&&a| a > 0.0); // banned
    let _ = [1.0f64].par_iter().position_any(|&a| a > 0.0); // banned
    let _ = [1.0f64].par_iter().try_for_each(|_| Some(())); // banned
    let _ = [1.0f64].par_iter().try_for_each_with((), |_, _| None); // banned
    let _ = [Some(1.0f64)].into_par_iter().while_some().count(); // banned
    let _ = [1.0f64].par_iter().take_any(1).count(); // banned
    let _ = [1.0f64].par_iter().skip_any_while(|&&a| a > 0.0).count(); // banned
    let _ = [1.0f64].iter().par_bridge().count(); // banned
    let _ = glam::DQuat::from_rotation_arc_colinear(glam::DVec3::X, glam::DVec3::Y); // banned
    let _ = spade::AngleLimit::from_rad(0.5); // banned
    let _ = std::collections::HashMap::<u32, u32>::new(); // banned
    let _ = std::collections::HashSet::<u32>::new(); // banned
    let p = vec![
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(0.0, 1.0),
    ];
    let _ = DelaunayTriangulation::<Point2<f64>>::bulk_load(p.clone()); // banned
    let _ = DelaunayTriangulation::<Point2<f64>>::bulk_load_stable(p.clone()); // banned
    let _ = ConstrainedDelaunayTriangulation::<Point2<f64>>::bulk_load_cdt(p.clone(), vec![]); // banned
    let _ = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(p, vec![], |_| {}); // banned
    let _ = spade::AngleLimit::from_deg(30.0); // banned
}
