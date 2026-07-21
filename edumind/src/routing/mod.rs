//! Deterministic, hot-reloadable channel-to-module routing.

pub mod resolver;
pub mod router;
pub mod session_key;
pub mod types;

pub use resolver::{ResolvedAgentRoute, RouteResolver};
pub use router::ModuleRouter;
pub use session_key::stable_session_key;
pub use types::{RouteRequest, RouteResolution, RouteRule, RouteTarget, RoutingTable};
