use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;

use super::auth::token::AgentToken;
use super::types::{AgentDescriptor, UsageInfo, UsageWindow, normalize_quota_display_name};

pub trait AgentIdentity: Send + Sync {
    fn descriptor(&self) -> &'static AgentDescriptor;
}

#[async_trait]
pub trait AgentAuthCapability: Send + Sync {
    async fn login(&self) -> Result<()>;
    async fn load_saved_token(&self) -> Result<Option<AgentToken>>;
    async fn refresh_if_needed(&self, token: &mut AgentToken) -> Result<()>;
}

#[async_trait]
pub trait AgentUsageCapability: Send + Sync {
    async fn get_usage(&self, token: &AgentToken) -> Result<UsageInfo>;
    async fn get_usage_raw(&self, token: &AgentToken) -> Result<Value>;

    fn quota_name(&self, window: &UsageWindow) -> String {
        window.name.clone()
    }

    fn display_quota_name(&self, window: &UsageWindow) -> String {
        normalize_quota_display_name(&window.name)
    }
}

pub trait Agent: AgentIdentity + Send + Sync {
    fn auth_capability(&self) -> Option<&dyn AgentAuthCapability> {
        None
    }

    fn usage_capability(&self) -> Option<&dyn AgentUsageCapability> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Agent, AgentDescriptor, AgentIdentity};

    struct IdentityOnlyAgent;

    impl AgentIdentity for IdentityOnlyAgent {
        fn descriptor(&self) -> &'static AgentDescriptor {
            static DESCRIPTOR: AgentDescriptor = AgentDescriptor {
                id: "identity-only",
                display_name: "Identity Only",
                token_file_name: "identity_only.json",
            };
            &DESCRIPTOR
        }
    }

    impl Agent for IdentityOnlyAgent {}

    #[test]
    fn default_capabilities_are_optional() {
        let agent = IdentityOnlyAgent;
        assert!(agent.auth_capability().is_none());
        assert!(agent.usage_capability().is_none());
        assert_eq!(agent.descriptor().id, "identity-only");
    }
}
