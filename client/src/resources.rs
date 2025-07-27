use bevy::prelude::*;
use bevy_quinnet::client::connection::ConnectionLocalId;
use protocol::messages::{InputState, Stance, WorldSnapshot};
use std::collections::{HashSet, VecDeque};

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