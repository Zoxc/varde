//! Turning a document's camera to a new view.

use std::time::Duration;

use iced::time::Instant;
use varde_render::Camera;

use super::Doc;

/// How long the camera takes to turn to a new view, like the UI mock.
pub(crate) const CAMERA_ANIMATION: Duration = Duration::from_millis(650);

/// The camera moving to a new view, eased out.
pub(crate) struct CameraAnimation {
    pub(crate) from: Camera,
    pub(crate) to: Camera,
    pub(crate) start: Instant,
}

impl CameraAnimation {
    /// The camera at `now`, or `None` once the animation is over.
    pub(crate) fn at(&self, now: Instant) -> Option<Camera> {
        let t = now.saturating_duration_since(self.start).as_secs_f32()
            / CAMERA_ANIMATION.as_secs_f32();
        (t < 1.0).then(|| {
            let eased = 1.0 - (1.0 - t).powi(3);
            self.from.lerp(&self.to, eased)
        })
    }
}

impl Doc {
    pub(crate) fn animate_camera(&mut self, to: Camera) {
        self.animation = Some(CameraAnimation {
            from: self.camera,
            to,
            start: Instant::now(),
        });
    }

    /// Moves the camera along its animation to where it is at `now`.
    pub(crate) fn animation_frame(&mut self, now: Instant) {
        if let Some(animation) = self.animation.take() {
            match animation.at(now) {
                Some(camera) => {
                    self.camera = camera;
                    self.animation = Some(animation);
                }
                None => self.camera = animation.to,
            }
        }
    }
}
