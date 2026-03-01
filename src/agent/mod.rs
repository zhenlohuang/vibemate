pub mod auth;
pub mod impls;
mod registry;
mod traits;
mod types;

pub use registry::global_agent_registry;
pub use traits::{Agent, AgentAuthCapability, AgentIdentity, AgentUsageCapability};
pub use types::{AgentDescriptor, UsageInfo, UsageWindow};
