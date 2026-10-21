//! Built-in devices and the registry that describes them.

mod filter;
mod gain;
mod noise;
mod osc;

use crate::ParamSpec;
use serde::Serialize;

pub struct Ctx {
    pub sample_rate: f32,
}

/// One block of work for a device. Every slice holds at least `frames` values.
pub struct Block<'a> {
    pub frames: usize,
    /// `params[i]` holds one smoothed value per frame for the device's i-th [`ParamSpec`].
    pub params: &'a [Vec<f32>],
    pub in_l: &'a [f32],
    pub in_r: &'a [f32],
    pub out_l: &'a mut [f32],
    pub out_r: &'a mut [f32],
}

/// DSP for one node. Implementations must be deterministic: output may depend only on
/// their inputs, their parameters and state built from those, never on time or block size.
pub trait Processor: Send {
    fn process(&mut self, ctx: &Ctx, block: Block<'_>);
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub kind: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamSpec],
}

pub static DEVICES: &[DeviceInfo] = &[
    DeviceInfo {
        kind: "osc",
        description: "Band-limited oscillator morphing from sine to saw. Ignores its input.",
        params: osc::PARAMS,
    },
    DeviceInfo {
        kind: "noise",
        description: "Seeded white noise. Ignores its input.",
        params: noise::PARAMS,
    },
    DeviceInfo {
        kind: "filter",
        description: "State-variable filter (lowpass, bandpass or highpass).",
        params: filter::PARAMS,
    },
    DeviceInfo {
        kind: "gain",
        description: "Gain and equal-power pan.",
        params: gain::PARAMS,
    },
    DeviceInfo {
        kind: "output",
        description: "Sums its inputs. The master node is an output.",
        params: &[],
    },
];

pub fn info(kind: &str) -> Option<&'static DeviceInfo> {
    DEVICES.iter().find(|d| d.kind == kind)
}

/// Creates the processor for `kind`. `seed` is unique per node and stable across renders.
pub fn create(kind: &str, seed: u64) -> Option<Box<dyn Processor>> {
    Some(match kind {
        "osc" => Box::new(osc::Osc::default()),
        "noise" => Box::new(noise::Noise::new(seed)),
        "filter" => Box::new(filter::Svf::default()),
        "gain" => Box::new(gain::Gain),
        "output" => Box::new(Passthrough),
        _ => return None,
    })
}

struct Passthrough;

impl Processor for Passthrough {
    fn process(&mut self, _ctx: &Ctx, b: Block<'_>) {
        b.out_l[..b.frames].copy_from_slice(&b.in_l[..b.frames]);
        b.out_r[..b.frames].copy_from_slice(&b.in_r[..b.frames]);
    }
}
