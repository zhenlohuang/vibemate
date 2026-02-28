pub mod auth;
pub mod impls;
mod registry;
mod traits;
mod types;

pub use registry::global_agent_registry;
pub use traits::{Agent, AgentAuthCapability, AgentIdentity, AgentUsageCapability};
pub use types::{normalize_quota_display_name, AgentDescriptor, UsageInfo, UsageWindow};
