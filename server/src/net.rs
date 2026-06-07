use bevy_quinnet::shared::channels::{ChannelConfig, SendChannelsConfiguration};
use protocol::channels::{CHANNELS, Reliability};

/// Собираем `SendChannelsConfiguration` из описания протокола
pub fn channels_config() -> SendChannelsConfiguration {
    let kinds = CHANNELS.iter().map(|desc| match desc.reliability {
        Reliability::OrderedReliable { max_frame_size } =>
            ChannelConfig::OrderedReliable { max_frame_size },
        Reliability::UnorderedReliable { max_frame_size } =>
            ChannelConfig::UnorderedReliable { max_frame_size },
    }).collect::<Vec<_>>();

    SendChannelsConfiguration::from_configs(kinds).expect("invalid channel config")
}