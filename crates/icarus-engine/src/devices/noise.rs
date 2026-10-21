use super::{Block, Ctx, Processor};
use crate::{Curve, ParamSpec};

pub static PARAMS: &[ParamSpec] = &[ParamSpec {
    id: "level",
    name: "Level",
    unit: "",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    curve: Curve::Linear,
    tag: "level.linear",
    description: "Output amplitude.",
}];

/// xorshift64* generator, seeded per node so two renders produce the same noise.
pub struct Noise {
    state: u64,
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next(&mut self) -> f32 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let bits = self.state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40;
        (bits as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

impl Processor for Noise {
    fn process(&mut self, _ctx: &Ctx, b: Block<'_>) {
        for i in 0..b.frames {
            let level = b.params[0][i];
            b.out_l[i] = self.next() * level;
            b.out_r[i] = self.next() * level;
        }
    }
}
