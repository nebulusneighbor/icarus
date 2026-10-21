use super::{Block, Ctx, Processor};
use crate::{Curve, ParamSpec};

pub static PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "gain",
        name: "Gain",
        unit: "dB",
        min: -60.0,
        max: 12.0,
        default: 0.0,
        curve: Curve::Linear,
        tag: "level.db",
        description: "Volume change. +6 dB roughly doubles amplitude.",
    },
    ParamSpec {
        id: "pan",
        name: "Pan",
        unit: "",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        curve: Curve::Linear,
        tag: "stereo.pan",
        description: "-1 left, 0 centre, 1 right (equal power).",
    },
];

pub struct Gain;

impl Processor for Gain {
    fn process(&mut self, _ctx: &Ctx, b: Block<'_>) {
        for i in 0..b.frames {
            let amp = 10f32.powf(b.params[0][i] / 20.0);
            let angle = (b.params[1][i] + 1.0) * std::f32::consts::FRAC_PI_4;
            b.out_l[i] = b.in_l[i] * amp * angle.cos() * std::f32::consts::SQRT_2;
            b.out_r[i] = b.in_r[i] * amp * angle.sin() * std::f32::consts::SQRT_2;
        }
    }
}
