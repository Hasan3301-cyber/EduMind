use crate::{
    agent::{AgentProfile, AgentRegistry},
    infra::Result,
};

use super::{ModuleRouter, RouteRequest, RouteResolution};

/// A route resolution paired with its enabled agent profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAgentRoute {
    pub route: RouteResolution,
    pub agent: AgentProfile,
}

/// Resolves channel routes and verifies the chosen agent is enabled.
#[derive(Clone, Debug)]
pub struct RouteResolver {
    router: ModuleRouter,
    agents: AgentRegistry,
}

impl RouteResolver {
    /// Creates a resolver from the active router and agent registry.
    #[must_use]
    pub fn new(router: ModuleRouter, agents: AgentRegistry) -> Self {
        Self { router, agents }
    }

    /// Resolves an incoming route request and its configured agent.
    pub fn resolve(&self, request: &RouteRequest) -> Result<ResolvedAgentRoute> {
        let route = self.router.resolve(request)?;
        let agent = self.agents.resolve(Some(&route.target.agent_id))?;
        Ok(ResolvedAgentRoute { route, agent })
    }

    /// Returns a clone of the underlying hot-reloadable router.
    #[must_use]
    pub fn router(&self) -> ModuleRouter {
        self.router.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::RouteResolver;
    use crate::{
        agent::AgentRegistry,
        config::EduMindConfig,
        routing::{ModuleRouter, RouteRequest, RouteTarget, RoutingTable},
    };

    #[test]
    fn rejects_routes_that_target_a_missing_agent() {
        let router = ModuleRouter::from_table(RoutingTable {
            default: RouteTarget {
                module_id: "student-os".to_owned(),
                agent_id: "missing".to_owned(),
            },
            routes: Vec::new(),
        })
        .unwrap();
        let resolver = RouteResolver::new(
            router,
            AgentRegistry::from_config(&EduMindConfig::default()).unwrap(),
        );

        assert!(
            resolver
                .resolve(&RouteRequest {
                    channel: "desktop".to_owned(),
                    ..RouteRequest::default()
                })
                .is_err()
        );
    }
}
