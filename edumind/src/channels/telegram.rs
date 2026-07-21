use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    channels::{
        ChannelDispatch, ChannelKind, ChannelManager, ChannelMessageHandler, InboundChannelMessage,
    },
    infra::Result,
};

/// Conversation type reported by a Telegram update source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelegramConversationKind {
    Direct,
    Group,
}

/// Provider-neutral Telegram update consumed by the polling adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramUpdate {
    pub update_id: String,
    pub account_id: String,
    pub chat_id: i64,
    pub sender_id: i64,
    pub conversation_kind: TelegramConversationKind,
    pub text: String,
    pub received_at: DateTime<Utc>,
}

impl TelegramUpdate {
    /// Normalizes an update into the common channel message envelope.
    #[must_use]
    pub fn into_inbound_message(self) -> InboundChannelMessage {
        let chat_id = self.chat_id.to_string();
        InboundChannelMessage {
            id: format!("telegram:{}", self.update_id),
            channel: ChannelKind::Telegram,
            account_id: self.account_id,
            peer_id: chat_id.clone(),
            guild_id: (self.conversation_kind == TelegramConversationKind::Group)
                .then_some(chat_id),
            team_id: None,
            sender_id: Some(self.sender_id),
            chat_id: Some(self.chat_id),
            content: self.text,
            received_at: self.received_at,
        }
    }
}

/// Source adapter implemented by a Telegram client, including a teloxide poller.
#[async_trait]
pub trait TelegramUpdateSource: Send + Sync {
    /// Returns the next update, or `None` after a clean polling cycle completes.
    async fn next_update(&self) -> Result<Option<TelegramUpdate>>;
}

/// Bridges a Telegram polling source into the common route and reply pipeline.
#[derive(Clone, Debug, Default)]
pub struct TelegramPollingChannel;

impl TelegramPollingChannel {
    /// Pulls and dispatches one update from a source client.
    pub async fn poll_once<S, H>(
        &self,
        source: &S,
        manager: &ChannelManager,
        handler: &H,
    ) -> Result<Option<ChannelDispatch>>
    where
        S: TelegramUpdateSource + ?Sized,
        H: ChannelMessageHandler + ?Sized,
    {
        let update = match source.next_update().await {
            Ok(update) => update,
            Err(error) => {
                manager.report_failure(ChannelKind::Telegram, error.to_string())?;
                return Err(error);
            }
        };
        let Some(update) = update else {
            return Ok(None);
        };
        manager
            .dispatch(update.into_inbound_message(), handler)
            .await
            .map(Some)
    }

    /// Polls until a caller-owned stop flag is raised or the source ends its cycle.
    pub async fn poll_until_stopped<S, H>(
        &self,
        source: &S,
        manager: &ChannelManager,
        handler: &H,
        stop: &AtomicBool,
    ) -> Result<usize>
    where
        S: TelegramUpdateSource + ?Sized,
        H: ChannelMessageHandler + ?Sized,
    {
        let mut dispatched = 0;
        while !stop.load(Ordering::Acquire) {
            if self.poll_once(source, manager, handler).await?.is_none() {
                break;
            }
            dispatched += 1;
        }
        Ok(dispatched)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{TelegramConversationKind, TelegramUpdate};

    #[test]
    fn group_updates_preserve_the_group_as_a_route_selector() {
        let message = TelegramUpdate {
            update_id: "99".to_owned(),
            account_id: "study-bot".to_owned(),
            chat_id: -100_123,
            sender_id: 42,
            conversation_kind: TelegramConversationKind::Group,
            text: "Explain this topic".to_owned(),
            received_at: Utc::now(),
        }
        .into_inbound_message();

        assert_eq!(message.id, "telegram:99");
        assert_eq!(message.guild_id.as_deref(), Some("-100123"));
        assert_eq!(message.sender_id, Some(42));
    }
}
