use crate::sys::id::WidgetId;
use crate::ui::types::Rect;

const MAX_SUBSTEP_DT: f32 = 0.004;

/// Velocity-preserving damped harmonic spring for smooth UI motion and mid-flight reversals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringState {
    /// Current interpolated spring position.
    pub value: f32,
    /// Current spring velocity in units per second.
    pub velocity: f32,
    /// Target equilibrium value.
    pub target: f32,
}

impl SpringState {
    /// Creates a settled spring initialized at `initial`.
    pub const fn new(initial: f32) -> Self {
        Self {
            value: initial,
            velocity: 0.0,
            target: initial,
        }
    }

    /// Advances the spring over `dt` seconds. Returns `true` while motion remains active.
    pub fn step(&mut self, dt: f32, stiffness: f32, damping: f32) -> bool {
        if dt <= 0.0 || !dt.is_finite() {
            return !self.is_settled();
        }
        let dt = dt.clamp(0.0005, 0.05);
        let steps = (dt / MAX_SUBSTEP_DT).ceil().max(1.0) as u32;
        let sub_dt = dt / steps as f32;
        let k = stiffness.max(1.0);
        let c = damping.max(0.1);

        for _ in 0..steps {
            let accel = -k * (self.value - self.target) - c * self.velocity;
            self.velocity += accel * sub_dt;
            self.value += self.velocity * sub_dt;
        }

        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }

    /// Returns `true` when position and velocity have converged to `target`.
    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.005
    }
}

/// Critically-damped duration tween preserving velocity on mid-flight target changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TweenState {
    /// Underlying critically-damped spring state.
    pub spring: SpringState,
    /// Nominal convergence duration in milliseconds.
    pub duration_ms: f32,
}

impl TweenState {
    /// Creates a settled tween initialized at `initial`.
    pub const fn new(initial: f32, duration_ms: f32) -> Self {
        Self {
            spring: SpringState::new(initial),
            duration_ms,
        }
    }

    /// Advances the tween by `dt` seconds. Returns `true` while active.
    pub fn step(&mut self, dt: f32) -> bool {
        let dur_s = (self.duration_ms.max(1.0)) * 0.001;
        let omega = 6.0 / dur_s;
        let stiffness = omega * omega;
        let damping = 2.0 * omega;
        self.spring.step(dt, stiffness, damping)
    }
}

/// FLIP layout transition state interpolating position and size between frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlipState {
    /// X position spring.
    pub x: SpringState,
    /// Y position spring.
    pub y: SpringState,
    /// Width spring.
    pub w: SpringState,
    /// Height spring.
    pub h: SpringState,
}

impl FlipState {
    /// Creates a FLIP state anchored at `rect`.
    pub const fn new(rect: Rect) -> Self {
        Self {
            x: SpringState::new(rect.x),
            y: SpringState::new(rect.y),
            w: SpringState::new(rect.w),
            h: SpringState::new(rect.h),
        }
    }

    /// Updates target bounds without discarding current motion velocity.
    pub fn set_target(&mut self, rect: Rect) {
        self.x.target = rect.x;
        self.y.target = rect.y;
        self.w.target = rect.w;
        self.h.target = rect.h;
    }

    /// Advances all four bounds over `dt` seconds. Returns `true` while animating.
    pub fn step(&mut self, dt: f32) -> bool {
        let kx = 360.0;
        let cx = 36.0;
        let a = self.x.step(dt, kx, cx);
        let b = self.y.step(dt, kx, cx);
        let c = self.w.step(dt, kx, cx);
        let d = self.h.step(dt, kx, cx);
        a || b || c || d
    }

    /// Returns the interpolated rectangle.
    pub fn rect(&self) -> Rect {
        Rect::new(
            self.x.value,
            self.y.value,
            self.w.value.max(0.0),
            self.h.value.max(0.0),
        )
    }
}

/// Active pointer click ripple state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RippleEffect {
    /// Widget ID owning the ripple animation.
    pub id: WidgetId,
    /// Logical press origin `[x, y]` of the expanding wave.
    pub center: [f32; 2],
    /// Normalized animation progress (`0.0..=1.0`).
    pub progress: f32,
}

#[cfg(test)]
mod tests {
    use super::{FlipState, SpringState, TweenState};
    use crate::ui::types::Rect;

    #[test]
    fn spring_preserves_velocity_on_reversal_and_settles() {
        let mut s = SpringState::new(0.0);
        s.target = 100.0;
        for _ in 0..10 {
            assert!(s.step(0.016, 280.0, 26.0));
        }
        let vel_before_reversal = s.velocity;
        assert!(vel_before_reversal > 10.0);

        // Reverse target mid-flight: velocity must carry smoothly without teleporting.
        s.target = 0.0;
        assert!(s.step(0.016, 280.0, 26.0));
        assert!(s.velocity < vel_before_reversal);
        assert!(s.value > 0.0);

        // Run until settled.
        let mut active = true;
        for _ in 0..400 {
            active = s.step(0.016, 280.0, 26.0);
            if !active {
                break;
            }
        }
        assert!(!active);
        assert_eq!(s.value, 0.0);
        assert_eq!(s.velocity, 0.0);
    }

    #[test]
    fn tween_and_flip_converge_cleanly() {
        let mut tw = TweenState::new(0.0, 150.0);
        tw.spring.target = 1.0;
        for _ in 0..200 {
            if !tw.step(0.016) {
                break;
            }
        }
        assert!((tw.spring.value - 1.0).abs() < 0.002);

        let mut flip = FlipState::new(Rect::new(0.0, 0.0, 100.0, 40.0));
        flip.set_target(Rect::new(120.0, 40.0, 200.0, 60.0));
        assert!(flip.step(0.016));
        assert!(flip.rect().x > 0.0 && flip.rect().x < 120.0);
        for _ in 0..300 {
            if !flip.step(0.016) {
                break;
            }
        }
        assert!((flip.rect().x - 120.0).abs() < 0.01);
        assert!((flip.rect().w - 200.0).abs() < 0.01);
    }
}
