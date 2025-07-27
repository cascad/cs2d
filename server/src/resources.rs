use bevy::{
    math::Vec2,
    prelude::{Resource, Timer},
};
use protocol::messages::InputState;
use std::collections::{HashMap, VecDeque};

#[derive(Default, Clone)]
pub struct PlayerState {
    pub pos: Vec2,
    pub rot: f32,
    pub stance: protocol::messages::Stance,
    pub hp: i32,
}

#[derive(Resource, Default)]
pub struct PlayerStates(pub HashMap<u64, PlayerState>);

#[derive(Resource)]
pub struct ServerTickTimer(pub Timer);

#[derive(Resource)]
pub struct SnapshotHistory {
    pub buf: VecDeque<(f64, HashMap<u64, PlayerState>)>,
    pub cap: usize,
}
impl Default for SnapshotHistory {
    fn default() -> Self {
        Self {
            buf: VecDeque::with_capacity(120),
            cap: 120,
        }
    }
}

#[derive(Resource, Default)]
pub struct PendingInputs(pub HashMap<u64, VecDeque<InputState>>);

#[derive(Resource, Default)]
pub struct AppliedSeqs(pub HashMap<u64, u32>);

#[derive(Resource, Default)]
pub struct LastHeard(pub HashMap<u64, f64>); // client_id → time (secs)
