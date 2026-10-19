//! Calls clippy.toml bans, one per line marked `// banned`, so tools/ci.sh
//! can check each is refused: it runs clippy on this file with
//! `--cfg ban_probe` and expects a disallowed-method or -type diagnostic on
//! exactly the marked lines. Empty in every other build.
//!
//! `primitive_paths` names every ban on a method of `f64` or `f32` by
//! its path, one per line: clippy can't report such a path that names
//! nothing, but this file doesn't build with one, and tools/ci.sh checks
//! each such path in clippy.toml has its line here.

#![cfg(ban_probe)]
#![allow(deprecated, unused_imports, reason = "only the bans matter here")]

use rayon::prelude::*;
use spade::{ConstrainedDelaunayTriangulation, DelaunayTriangulation, Point2, Triangulation};

#[test]
fn primitive_paths() {
    let _ = f64::sin; // banned
    let _ = f64::cos; // banned
    let _ = f64::sin_cos; // banned
    let _ = f64::tan; // banned
    let _ = f64::asin; // banned
    let _ = f64::acos; // banned
    let _ = f64::atan; // banned
    let _ = f64::atan2; // banned
    let _ = f64::sinh; // banned
    let _ = f64::cosh; // banned
    let _ = f64::tanh; // banned
    let _ = f64::asinh; // banned
    let _ = f64::acosh; // banned
    let _ = f64::atanh; // banned
    let _ = f64::exp; // banned
    let _ = f64::exp2; // banned
    let _ = f64::exp_m1; // banned
    let _ = f64::ln; // banned
    let _ = f64::ln_1p; // banned
    let _ = f64::log; // banned
    let _ = f64::log2; // banned
    let _ = f64::log10; // banned
    let _ = f64::powf; // banned
    let _ = f64::powi; // banned
    let _ = f64::cbrt; // banned
    let _ = f64::hypot; // banned
    let _ = f32::sin; // banned
    let _ = f32::cos; // banned
    let _ = f32::sin_cos; // banned
    let _ = f32::tan; // banned
    let _ = f32::asin; // banned
    let _ = f32::acos; // banned
    let _ = f32::atan; // banned
    let _ = f32::atan2; // banned
    let _ = f32::sinh; // banned
    let _ = f32::cosh; // banned
    let _ = f32::tanh; // banned
    let _ = f32::asinh; // banned
    let _ = f32::acosh; // banned
    let _ = f32::atanh; // banned
    let _ = f32::exp; // banned
    let _ = f32::exp2; // banned
    let _ = f32::exp_m1; // banned
    let _ = f32::ln; // banned
    let _ = f32::ln_1p; // banned
    let _ = f32::log; // banned
    let _ = f32::log2; // banned
    let _ = f32::log10; // banned
    let _ = f32::powf; // banned
    let _ = f32::powi; // banned
    let _ = f32::cbrt; // banned
    let _ = f32::hypot; // banned
    let _ = f64::mul_add; // banned
    let _ = f32::mul_add; // banned
    let _ = f64::min; // banned
    let _ = f64::max; // banned
    let _ = f32::min; // banned
    let _ = f32::max; // banned
}

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
    let _ = glam::Vec3::ONE; // banned
    let _: Option<glam::Quat> = None; // banned
    let _ = glam::Affine3A::IDENTITY; // banned
    rayon::join(|| (), || ()); // banned
    rayon::scope(|_| ()); // banned
    let _ = rayon::current_num_threads(); // banned
    let _ = rayon::current_thread_index(); // banned
    let _ = rayon::ThreadPoolBuilder::new(); // banned
    let _ = std::time::Instant::now(); // banned
    let _ = std::time::SystemTime::now(); // banned
}
