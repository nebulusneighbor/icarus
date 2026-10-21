use serde::Serialize;

/// How a parameter's plain value maps onto a 0..1 control range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    Linear,
    /// Exponential mapping, for frequencies and times.
    Log,
    /// Integer choices `min..=max`; never smoothed.
    Stepped,
}

/// Everything an agent needs to know to use a parameter well.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub curve: Curve,
    /// Semantic tag shared across devices, e.g. `filter.cutoff`.
    pub tag: &'static str,
    /// What turning this control does to the sound.
    pub description: &'static str,
}

impl ParamSpec {
    pub fn clamp(&self, value: f64) -> f64 {
        let v = value.clamp(self.min, self.max);
        if self.curve == Curve::Stepped { v.round() } else { v }
    }

    pub fn to_normalized(&self, value: f64) -> f64 {
        let v = self.clamp(value);
        match self.curve {
            Curve::Log => (v / self.min).ln() / (self.max / self.min).ln(),
            Curve::Linear | Curve::Stepped => (v - self.min) / (self.max - self.min),
        }
    }

    pub fn from_normalized(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0);
        self.clamp(match self.curve {
            Curve::Log => self.min * (self.max / self.min).powf(n),
            Curve::Linear | Curve::Stepped => self.min + n * (self.max - self.min),
        })
    }
}
