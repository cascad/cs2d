use bevy::prelude::*;
use bevy_quinnet::client::connection::ConnectionLocalId;
use protocol::messages::{InputState, Stance, WorldSnapshot};
use std::collections::{HashMap, HashSet, VecDeque};

pub mod explosion_textures;
pub mod grenades;

#[derive(Resource)]
pub struct MyPlayer {
    pub id: u64,
    pub got: bool,
}

#[derive(Resource)]
pub struct TimeSync {
    pub offset: f64,
}

#[derive(Resource)]
pub struct SnapshotBuffer {
    pub snapshots: VecDeque<WorldSnapshot>,
    pub delay: f64,
}

#[derive(Resource)]
pub struct CurrentStance(pub Stance);

#[derive(Resource)]
pub struct SendTimer(pub Timer);

#[derive(Resource, Default)]
pub struct SpawnedPlayers(pub HashSet<u64>);

#[derive(Resource)]
pub struct SeqCounter(pub u32);

#[derive(Resource, Default)]
pub struct PendingInputsClient(pub VecDeque<InputState>);

#[derive(Resource, Default)]
pub struct CurrentConnId(pub Option<ConnectionLocalId>);

#[derive(Resource)]
pub struct HeartbeatTimer(pub Timer);

impl Default for HeartbeatTimer {
    fn default() -> Self {
        // шлём heartbeat каждую секунду
        HeartbeatTimer(Timer::from_seconds(1.0, TimerMode::Repeating))
    }
}

#[derive(Resource)]
pub struct ClientLatency {
    pub rtt: f64,
    pub offset: f64,  // серверное время = client_time + one_way + offset
    pub timer: Timer, // для пингования
}

impl Default for ClientLatency {
    fn default() -> Self {
        Self {
            rtt: 0.0,
            offset: 0.0,
            timer: Timer::from_seconds(1.0, TimerMode::Repeating),
        }
    }
}

#[derive(Resource, Default)]
pub struct ConnectedPlayers(pub HashSet<u64>);

#[derive(Resource, Default)]
/// Tracks players who are currently “dead” and should _not_ be spawned
pub struct DeadPlayers(pub HashSet<u64>);

#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

#[derive(Resource, Default)]
pub struct HpUiMap(pub HashMap<u64, Entity>);