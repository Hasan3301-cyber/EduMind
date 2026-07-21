//! Channel lifecycle management and provider-neutral message ingestion.

pub mod manager;
pub mod telegram;

pub use manager::{
    ChannelDispatch, ChannelKind, ChannelManager, ChannelMessageHandler, ChannelState,
    ChannelStatus, InboundChannelMessage, OutboundChannelMessage, RoutedChannelMessage,
};
pub use telegram::{
    TelegramConversationKind, TelegramPollingChannel, TelegramUpdate, TelegramUpdateSource,
};
