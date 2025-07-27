use bevy_quinnet::shared::channels::{ChannelKind, ChannelsConfiguration};
use protocol::channels::{CHANNELS, Reliability};

/// Собираем `ChannelsConfiguration` из описания протокола
pub fn channels_config() -> ChannelsConfiguration {
    let kinds = CHANNELS.iter().map(|desc| match desc.reliability {
        Reliability::OrderedReliable { max_frame_size } =>
            ChannelKind::OrderedReliable { max_frame_size },
        Reliability::UnorderedReliable { max_frame_size } =>
            ChannelKind::UnorderedReliable { max_frame_size },
    }).collect::<Vec<_>>();

    ChannelsConfiguration::from_types(kinds).expect("invalid channel config")
}