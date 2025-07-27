use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ----- Client → Server -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum C2S {
    Input(InputState),
    Shoot(ShootEvent),
    Heartbeat,
    Goodbye,
}

// ----- Server → Client -----
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum S2C {
    Snapshot(WorldSnapshot),
    ShootFx(ShootFx),
    PlayerLeft(u64),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InputState {
    pub seq: u32,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub rotation: f32,
    pub stance: Stance,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootEvent {
    pub shooter_id: u64,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShootFx {
    pub shooter_id: u64,
    pub from: Vec2,
    pub dir: Vec2,
    pub timestamp: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Stance {
    Standing,
    Crouching,
    Prone,
}
impl Default for Stance {
    fn default() -> Self {
        Stance::Standing
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub stance: Stance,
    pub hp: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldSnapshot {
    pub players: Vec<PlayerSnapshot>,
    pub server_time: f64,
    pub last_input_seq: HashMap<u64, u32>,
}
