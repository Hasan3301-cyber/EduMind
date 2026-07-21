//! EduMind gateway library.
//!
//! The gateway provides local-first configuration, memory, gateway, and agent
//! orchestration contracts from the EduMind master build plan.

pub mod agent;
pub mod channels;
pub mod config;
pub mod gateway;
pub mod infra;
pub mod jobs;
pub mod mcp;
pub mod memory;
pub mod research;
pub mod routing;
pub mod runs;
pub mod runtime_tools;
pub mod secrets;
pub mod security;
pub mod student;
pub mod study;

pub use gateway::{GatewayBootstrap, GatewayMode};
