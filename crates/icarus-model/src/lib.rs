//! Data shared by every Icarus client: the project document and the control protocol.
//!
//! The GUI, agents and hardware controllers all speak [`Request`]. Nothing a human can do
//! is out of reach of an agent, because there is no other way to change a project.

mod project;
mod protocol;

pub use project::*;
pub use protocol::*;
