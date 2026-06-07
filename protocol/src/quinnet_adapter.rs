// Адаптер для построения SendChannelsConfiguration из описания протокола
use bevy_quinnet::shared::channels::{ChannelConfig, SendChannelsConfiguration};
use crate::channels::{CHANNELS, Reliability};

pub fn build_channels_config() -> SendChannelsConfiguration {
    let kinds = CHANNELS.iter().map(|desc| match desc.reliability {
        Reliability::OrderedReliable { max_frame_size } =>
            ChannelConfig::OrderedReliable { max_frame_size },
        Reliability::UnorderedReliable { max_frame_size } =>
            ChannelConfig::UnorderedReliable { max_frame_size },
    }).collect::<Vec<_>>();
    SendChannelsConfiguration::from_configs(kinds).expect("invalid channel config")
}