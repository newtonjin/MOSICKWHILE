//! One Euro Filter — low lag when moving, strong jitter rejection when still.
//! Casiez et al., CHI 2012. Ideal for gaze / FOV clear-zone tracking.

#[derive(Debug, Clone)]
pub struct OneEuro {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x_prev: Option<f32>,
    dx_prev: f32,
}

impl OneEuro {
    /// Gaze-tuned: strong stillness filter (video flicker / pupil noise).
    pub fn gaze() -> Self {
        Self {
            min_cutoff: 0.70,
            beta: 0.38,
            d_cutoff: 0.85,
            x_prev: None,
            dx_prev: 0.0,
        }
    }

    /// Clear-zone: calm when watching; still tracks real saccades.
    pub fn clear_zone() -> Self {
        Self {
            min_cutoff: 0.55,
            beta: 0.32,
            d_cutoff: 0.75,
            x_prev: None,
            dx_prev: 0.0,
        }
    }

    pub fn reset(&mut self, x: f32) {
        self.x_prev = Some(x);
        self.dx_prev = 0.0;
    }

    pub fn filter(&mut self, x: f32, dt: f32) -> f32 {
        let dt = dt.clamp(0.008, 0.1);
        let Some(x_prev) = self.x_prev else {
            self.x_prev = Some(x);
            return x;
        };

        let dx = (x - x_prev) / dt;
        let edx = lowpass(self.dx_prev, dx, alpha(dt, self.d_cutoff));
        self.dx_prev = edx;

        let cutoff = self.min_cutoff + self.beta * edx.abs();
        let out = lowpass(x_prev, x, alpha(dt, cutoff));
        self.x_prev = Some(out);
        out
    }
}

fn alpha(dt: f32, cutoff: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(0.05));
    1.0 / (1.0 + tau / dt)
}

fn lowpass(prev: f32, x: f32, a: f32) -> f32 {
    prev + a.clamp(0.0, 1.0) * (x - prev)
}
