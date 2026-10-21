use super::{Block, Ctx, Processor};
use crate::{Curve, ParamSpec};

pub static PARAMS: &[ParamSpec] = &[
    ParamSpec {
        id: "cutoff",
        name: "Cutoff",
        unit: "Hz",
        min: 20.0,
        max: 20_000.0,
        default: 1_000.0,
        curve: Curve::Log,
        tag: "filter.cutoff",
        description: "Corner frequency. In lowpass mode, lower values sound darker and duller.",
    },
    ParamSpec {
        id: "resonance",
        name: "Resonance",
        unit: "",
        min: 0.0,
        max: 1.0,
        default: 0.2,
        curve: Curve::Linear,
        tag: "filter.resonance",
        description: "Emphasis at the cutoff. High values add a whistling peak and can ring.",
    },
    ParamSpec {
        id: "mode",
        name: "Mode",
        unit: "",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        curve: Curve::Stepped,
        tag: "filter.mode",
        description: "0 lowpass, 1 bandpass, 2 highpass.",
    },
];

/// Topology-preserving state-variable filter (Simper/Cytomic), one per channel.
#[derive(Default)]
pub struct Svf {
    ic1: [f32; 2],
    ic2: [f32; 2],
}

impl Processor for Svf {
    fn process(&mut self, ctx: &Ctx, b: Block<'_>) {
        let nyquist_guard = ctx.sample_rate * 0.49;
        for i in 0..b.frames {
            let cutoff = b.params[0][i].min(nyquist_guard);
            let k = 2.0 - 2.0 * b.params[1][i].min(0.98);
            let mode = b.params[2][i] as u32;
            let g = (std::f32::consts::PI * cutoff / ctx.sample_rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;
            for (ch, (input, out)) in [(b.in_l[i], &mut b.out_l[i]), (b.in_r[i], &mut b.out_r[i])]
                .into_iter()
                .enumerate()
            {
                let v3 = input - self.ic2[ch];
                let v1 = a1 * self.ic1[ch] + a2 * v3;
                let v2 = self.ic2[ch] + a2 * self.ic1[ch] + a3 * v3;
                self.ic1[ch] = 2.0 * v1 - self.ic1[ch];
                self.ic2[ch] = 2.0 * v2 - self.ic2[ch];
                *out = match mode {
                    0 => v2,
                    1 => v1,
                    _ => input - k * v1 - v2,
                };
            }
        }
    }
}
