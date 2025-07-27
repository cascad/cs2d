use crate::constants::{CH_C2S, CH_S2C};

/// Описание надёжности канала без привязки к Quinnet
#[derive(Clone, Copy, Debug)]
pub enum Reliability {
    OrderedReliable { max_frame_size: usize },
    UnorderedReliable { max_frame_size: usize },
}

/// Описание канала протокола
#[derive(Clone, Copy, Debug)]
pub struct ChannelDesc {
    pub id: u8,
    pub reliability: Reliability,
}

/// Настройка каналов (client → server, server → client)
pub const CHANNELS: &[ChannelDesc] = &[
    ChannelDesc { id: CH_C2S, reliability: Reliability::OrderedReliable { max_frame_size: 16_000 } },
    ChannelDesc { id: CH_S2C, reliability: Reliability::OrderedReliable { max_frame_size: 16_000 } },
];