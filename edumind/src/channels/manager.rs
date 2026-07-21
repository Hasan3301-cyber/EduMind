use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    config::types::ChannelsConfig,
    infra::{EduMindError, Result},
    routing::{ResolvedAgentRoute, RouteRequest, RouteResolver},
};

/// EduMind channel implementations supported by the manager.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Desktop,
    Telegram,
}

impl ChannelKind {
    const ALL: [Self; 2] = [Self::Desktop, Self::Telegram];

    /// Returns the configured channel identifier used by routing and agent policies.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Telegram => "telegram",
        }
    }
}

/// Current lifecycle state for a channel source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelState {
    Stopped,
    Running,
    Failed,
}

/// Observable status for one managed channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelStatus {
    pub channel: ChannelKind,
    pub state: ChannelState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl ChannelStatus {
    fn stopped(channel: ChannelKind) -> Self {
        Self {
            channel,
            state: ChannelState::Stopped,
            last_error: None,
        }
    }
}

/// A message normalized from an external chat source before routing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundChannelMessage {
    pub id: String,
    pub channel: ChannelKind,
    pub account_id: String,
    pub peer_id: String,
    pub guild_id: Option<String>,
    pub team_id: Option<String>,
    pub sender_id: Option<i64>,
    pub chat_id: Option<i64>,
    pub content: String,
    pub received_at: DateTime<Utc>,
}

impl InboundChannelMessage {
    /// Builds the route selectors for this source message.
    #[must_use]
    pub fn route_request(&self) -> RouteRequest {
        RouteRequest {
            channel: self.channel.as_str().to_owned(),
            account_id: Some(self.account_id.clone()),
            peer_id: Some(self.peer_id.clone()),
            guild_id: self.guild_id.clone(),
            team_id: self.team_id.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty()
            || self.account_id.trim().is_empty()
            || self.peer_id.trim().is_empty()
            || self.content.trim().is_empty()
        {
            return Err(EduMindError::Channel(
                "inbound messages require id, account_id, peer_id, and content".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A reply returned by the agent pipeline for delivery through the source channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundChannelMessage {
    pub channel: ChannelKind,
    pub account_id: String,
    pub peer_id: String,
    pub reply_to_message_id: String,
    pub content: String,
}

/// An inbound message paired with the selected module, agent, and session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutedChannelMessage {
    pub message: InboundChannelMessage,
    pub route: ResolvedAgentRoute,
}

/// The outcome of routing an inbound message through an agent message handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelDispatch {
    pub routed: RoutedChannelMessage,
    pub response: Option<OutboundChannelMessage>,
}

/// Adapter from a routed message into the agent chat pipeline.
#[async_trait]
pub trait ChannelMessageHandler: Send + Sync {
    /// Handles one routed message and optionally returns a channel reply.
    async fn handle(&self, message: RoutedChannelMessage)
    -> Result<Option<OutboundChannelMessage>>;
}

/// Coordinates lifecycle, policy checks, and route dispatch for all chat sources.
#[derive(Clone, Debug)]
pub struct ChannelManager {
    config: Arc<RwLock<ChannelsConfig>>,
    resolver: RouteResolver,
    statuses: Arc<Mutex<BTreeMap<ChannelKind, ChannelStatus>>>,
}

impl ChannelManager {
    /// Creates a stopped manager from independently valid channel configuration.
    pub fn new(config: ChannelsConfig, resolver: RouteResolver) -> Result<Self> {
        config.validate()?;
        let statuses = ChannelKind::ALL
            .into_iter()
            .map(|channel| (channel, ChannelStatus::stopped(channel)))
            .collect();
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            resolver,
            statuses: Arc::new(Mutex::new(statuses)),
        })
    }

    /// Starts each enabled channel and returns the resulting status snapshot.
    pub fn start(&self) -> Result<Vec<ChannelStatus>> {
        let config = self.config_snapshot()?;
        let mut statuses = self.statuses.lock().map_err(|error| {
            EduMindError::Channel(format!("channel status lock failed: {error}"))
        })?;
        for channel in ChannelKind::ALL {
            let status = statuses
                .get_mut(&channel)
                .expect("all supported channels have initialized statuses");
            if channel_enabled(&config, channel) {
                status.state = ChannelState::Running;
                status.last_error = None;
            } else {
                *status = ChannelStatus::stopped(channel);
            }
        }
        Ok(statuses.values().cloned().collect())
    }

    /// Stops a single source without changing its active configuration.
    pub fn stop(&self, channel: ChannelKind) -> Result<()> {
        let mut statuses = self.statuses.lock().map_err(|error| {
            EduMindError::Channel(format!("channel status lock failed: {error}"))
        })?;
        let status = statuses
            .get_mut(&channel)
            .expect("all supported channels have initialized statuses");
        *status = ChannelStatus::stopped(channel);
        Ok(())
    }

    /// Atomically applies channel configuration, stopping newly disabled sources.
    pub fn replace_config(&self, config: ChannelsConfig) -> Result<()> {
        config.validate()?;
        *self.config.write().map_err(|error| {
            EduMindError::Channel(format!("channel configuration lock failed: {error}"))
        })? = config.clone();

        let mut statuses = self.statuses.lock().map_err(|error| {
            EduMindError::Channel(format!("channel status lock failed: {error}"))
        })?;
        for channel in ChannelKind::ALL {
            if !channel_enabled(&config, channel) {
                *statuses
                    .get_mut(&channel)
                    .expect("all supported channels have initialized statuses") =
                    ChannelStatus::stopped(channel);
            }
        }
        Ok(())
    }

    /// Returns one channel's lifecycle status.
    pub fn status(&self, channel: ChannelKind) -> Result<ChannelStatus> {
        self.statuses
            .lock()
            .map_err(|error| EduMindError::Channel(format!("channel status lock failed: {error}")))?
            .get(&channel)
            .cloned()
            .ok_or_else(|| {
                EduMindError::Channel(format!("unsupported channel `{}`", channel.as_str()))
            })
    }

    /// Returns every status in stable channel order.
    pub fn statuses(&self) -> Result<Vec<ChannelStatus>> {
        self.statuses
            .lock()
            .map(|statuses| statuses.values().cloned().collect())
            .map_err(|error| EduMindError::Channel(format!("channel status lock failed: {error}")))
    }

    /// Records a recoverable source error without stopping the channel.
    pub fn record_error(&self, channel: ChannelKind, error: impl Into<String>) -> Result<()> {
        self.update_error(channel, error.into(), false)
    }

    /// Marks a source failed until it is explicitly started again.
    pub fn report_failure(&self, channel: ChannelKind, error: impl Into<String>) -> Result<()> {
        self.update_error(channel, error.into(), true)
    }

    /// Validates and resolves an inbound source message without invoking a model.
    pub fn route(&self, message: InboundChannelMessage) -> Result<RoutedChannelMessage> {
        message.validate()?;
        let config = self.config_snapshot()?;
        if !channel_enabled(&config, message.channel) {
            return Err(EduMindError::Channel(format!(
                "{} channel is disabled",
                message.channel.as_str()
            )));
        }
        if self.status(message.channel)?.state != ChannelState::Running {
            return Err(EduMindError::Channel(format!(
                "{} channel is not running",
                message.channel.as_str()
            )));
        }
        if message.channel == ChannelKind::Telegram {
            ensure_telegram_allowed(&config, &message)?;
        }

        let route = self.resolver.resolve(&message.route_request())?;
        if !route
            .agent
            .allowed_channels
            .iter()
            .any(|channel| channel == message.channel.as_str())
        {
            return Err(EduMindError::Channel(format!(
                "agent `{}` is not permitted on {}",
                route.agent.id,
                message.channel.as_str()
            )));
        }
        Ok(RoutedChannelMessage { message, route })
    }

    /// Routes a source message and delivers it to the agent chat pipeline adapter.
    pub async fn dispatch<H: ChannelMessageHandler + ?Sized>(
        &self,
        message: InboundChannelMessage,
        handler: &H,
    ) -> Result<ChannelDispatch> {
        let routed = self.route(message)?;
        let channel = routed.message.channel;
        match handler.handle(routed.clone()).await {
            Ok(response) => Ok(ChannelDispatch { routed, response }),
            Err(error) => {
                self.record_error(channel, error.to_string())?;
                Err(error)
            }
        }
    }

    fn config_snapshot(&self) -> Result<ChannelsConfig> {
        self.config
            .read()
            .map(|config| config.clone())
            .map_err(|error| {
                EduMindError::Channel(format!("channel configuration lock failed: {error}"))
            })
    }

    fn update_error(&self, channel: ChannelKind, error: String, failed: bool) -> Result<()> {
        let mut statuses = self.statuses.lock().map_err(|lock_error| {
            EduMindError::Channel(format!("channel status lock failed: {lock_error}"))
        })?;
        let status = statuses
            .get_mut(&channel)
            .expect("all supported channels have initialized statuses");
        status.last_error = Some(error);
        if failed {
            status.state = ChannelState::Failed;
        }
        Ok(())
    }
}

fn channel_enabled(config: &ChannelsConfig, channel: ChannelKind) -> bool {
    match channel {
        ChannelKind::Desktop => config.desktop.enabled,
        ChannelKind::Telegram => config.telegram.enabled,
    }
}

fn ensure_telegram_allowed(config: &ChannelsConfig, message: &InboundChannelMessage) -> Result<()> {
    let telegram = &config.telegram;
    if !telegram.allowed_user_ids.is_empty()
        && !message
            .sender_id
            .is_some_and(|sender_id| telegram.allowed_user_ids.contains(&sender_id))
    {
        return Err(EduMindError::Channel(
            "telegram sender is not in channels.telegram.allowed_user_ids".to_owned(),
        ));
    }
    if !telegram.allowed_chat_ids.is_empty()
        && !message
            .chat_id
            .is_some_and(|chat_id| telegram.allowed_chat_ids.contains(&chat_id))
    {
        return Err(EduMindError::Channel(
            "telegram chat is not in channels.telegram.allowed_chat_ids".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::Utc;

    use super::{
        ChannelKind, ChannelManager, ChannelMessageHandler, ChannelState, InboundChannelMessage,
        OutboundChannelMessage, RoutedChannelMessage,
    };
    use crate::{
        agent::AgentRegistry,
        config::EduMindConfig,
        infra::Result,
        routing::{ModuleRouter, RouteResolver, RoutingTable},
    };

    struct EchoHandler;

    #[async_trait]
    impl ChannelMessageHandler for EchoHandler {
        async fn handle(
            &self,
            message: RoutedChannelMessage,
        ) -> Result<Option<OutboundChannelMessage>> {
            Ok(Some(OutboundChannelMessage {
                channel: message.message.channel,
                account_id: message.message.account_id,
                peer_id: message.message.peer_id,
                reply_to_message_id: message.message.id,
                content: "Ready to study.".to_owned(),
            }))
        }
    }

    fn manager(config: &EduMindConfig) -> ChannelManager {
        let resolver = RouteResolver::new(
            ModuleRouter::from_table(RoutingTable::default()).unwrap(),
            AgentRegistry::from_config(config).unwrap(),
        );
        ChannelManager::new(config.channels.clone(), resolver).unwrap()
    }

    fn message(channel: ChannelKind) -> InboundChannelMessage {
        InboundChannelMessage {
            id: "message-1".to_owned(),
            channel,
            account_id: channel.as_str().to_owned(),
            peer_id: "learner-1".to_owned(),
            guild_id: None,
            team_id: None,
            sender_id: Some(7),
            chat_id: Some(9),
            content: "Help me plan revision.".to_owned(),
            received_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn desktop_messages_route_and_receive_pipeline_replies() {
        let config = EduMindConfig::default();
        let manager = manager(&config);
        manager.start().unwrap();

        let dispatch = manager
            .dispatch(message(ChannelKind::Desktop), &EchoHandler)
            .await
            .unwrap();

        assert_eq!(dispatch.routed.route.agent.id, "master");
        assert_eq!(dispatch.response.unwrap().content, "Ready to study.");
    }

    #[test]
    fn telegram_allowlists_require_every_configured_dimension() {
        let mut config = EduMindConfig::default();
        config.agents.list[0]
            .allowed_channels
            .push("telegram".to_owned());
        config.channels.telegram.enabled = true;
        config.channels.telegram.token = Some("token".to_owned());
        config.channels.telegram.allowed_user_ids = vec![7];
        config.channels.telegram.allowed_chat_ids = vec![9];
        let manager = manager(&config);
        manager.start().unwrap();

        assert!(manager.route(message(ChannelKind::Telegram)).is_ok());
        let mut denied = message(ChannelKind::Telegram);
        denied.sender_id = Some(8);

        assert!(manager.route(denied).is_err());
        assert_eq!(
            manager.status(ChannelKind::Telegram).unwrap().state,
            ChannelState::Running
        );
    }
}
