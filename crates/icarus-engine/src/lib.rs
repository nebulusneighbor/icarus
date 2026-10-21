//! The Icarus audio engine.
//!
//! The engine is deterministic: the same [`Project`](icarus_model::Project) always renders to
//! bit-identical audio, whatever block size it is processed in. Parameters and automation are
//! resolved per frame from the absolute sample position, devices hold no wall-clock state,
//! and every random source is seeded from the project. Agents rely on this to compare renders.

pub mod devices;
mod graph;
mod params;
mod realtime;
mod seed;

pub use graph::{CompileError, Graph, MAX_BLOCK, ParamAddr};
pub use params::{Curve, ParamSpec};
pub use realtime::{EngineHandle, EngineMsg, RealtimeEngine, realtime_pair};
pub use seed::fnv1a;
