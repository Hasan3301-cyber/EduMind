use std::net::SocketAddr;

pub mod agent;
pub mod auth;
pub mod chat;
pub mod class_notes;
pub mod collab;
pub mod group_study;
pub mod memory;
pub mod protocol;
pub mod research;
pub mod runs;
pub mod runtime;
pub mod scheduler;
pub mod server;
pub mod student_pages;
pub mod study;
pub mod wellness;

pub use auth::{AuthPrincipal, AuthService, Role};
pub use collab::CollaborationService;
pub use protocol::{
    ConnectParams, EventFrame, PROTOCOL_VERSION, ProtocolError, RequestFrame, ResponseFrame,
};
pub use scheduler::{ScheduledJobHandler, ScheduledJobInvocation, Scheduler};
pub use server::{
    AppState, Broadcaster, bind_is_loopback, bind_listener, build_router, ensure_secure_bind,
    serve, serve_with_shutdown,
};

/// Startup mode for the EduMind gateway.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GatewayMode {
    /// Runs the standalone command-line gateway.
    #[default]
    Standalone,
    /// Runs the gateway under the Tauri desktop lifecycle.
    Embedded,
}

/// Minimal configuration passed to the future gateway bootstrap service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayBootstrap {
    pub bind_addr: SocketAddr,
    pub mode: GatewayMode,
}

impl GatewayBootstrap {
    /// Creates an ephemeral loopback bootstrap suitable for local development.
    #[must_use]
    pub fn local(mode: GatewayMode) -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GatewayBootstrap, GatewayMode};

    #[test]
    fn local_bootstrap_uses_loopback_and_ephemeral_port() {
        let bootstrap = GatewayBootstrap::local(GatewayMode::Embedded);

        assert!(bootstrap.bind_addr.ip().is_loopback());
        assert_eq!(bootstrap.bind_addr.port(), 0);
        assert_eq!(bootstrap.mode, GatewayMode::Embedded);
    }
}
