use std::sync::OnceLock;

use super::impls::{claude, codex};
use super::traits::Agent;

pub struct AgentRegistry {
    agents: Vec<Box<dyn Agent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: vec![Box::new(codex::CodexAgent), Box::new(claude::ClaudeAgent)],
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Agent> + '_ {
        self.agents.iter().map(Box::as_ref)
    }

    pub fn get(&self, id: &str) -> Option<&dyn Agent> {
        self.iter().find(|agent| agent.descriptor().id == id)
    }

    pub fn supported_ids(&self) -> Vec<&'static str> {
        self.iter().map(|agent| agent.descriptor().id).collect()
    }
}

static GLOBAL_AGENT_REGISTRY: OnceLock<AgentRegistry> = OnceLock::new();

pub fn global_agent_registry() -> &'static AgentRegistry {
    GLOBAL_AGENT_REGISTRY.get_or_init(AgentRegistry::new)
}
