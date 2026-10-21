use super::{Block, Ctx, Processor};
use crate::{Curve, ParamSpec};

pub static PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "freq",
        name: "Frequency",
        unit: "Hz",
        min: 20.0,
        max: 20_000.0,
        default: 110.0,
        curve: Curve::Log,
        tag: "osc.pitch",
        description: "Pitch of the oscillator.",
    },
    ParamSpec {
        id: "shape",
        name: "Shape",
        unit: "",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        curve: Curve::Linear,
        tag: "osc.timbre",
        description: "0 is a pure sine, 1 a saw. Higher values add harmonics and brightness.",
    },
    ParamSpec {
        id: "level",
        name: "Level",
        unit: "",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        curve: Curve::Linear,
        tag: "level.linear",
        description: "Output amplitude.",
    },
];

#[derive(Default)]
pub struct Osc {
    phase: f64,
}

/// PolyBLEP correction that removes most aliasing from the saw's discontinuity.
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

impl Processor for Osc {
    fn process(&mut self, ctx: &Ctx, b: Block<'_>) {
        let sr = f64::from(ctx.sample_rate);
        for i in 0..b.frames {
            let freq = f64::from(b.params[0][i]);
            let shape = f64::from(b.params[1][i]);
            let level = f64::from(b.params[2][i]);
            let dt = (freq / sr).min(0.5);
            let sine = (self.phase * std::f64::consts::TAU).sin();
            let saw = 2.0 * self.phase - 1.0 - poly_blep(self.phase, dt);
            let y = (level * (sine + shape * (saw - sine))) as f32;
            b.out_l[i] = y;
            b.out_r[i] = y;
            self.phase += dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}
