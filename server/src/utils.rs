use std::collections::{HashMap, VecDeque};
use protocol::messages::ShootEvent;
use crate::resources::PlayerState;
use bevy::prelude::Vec2;
use crate::constants::{MAX_RAY_LEN, HITBOX_RADIUS};

/// Сохраняем историю состояний
pub fn push_history(
    history: &mut crate::resources::SnapshotHistory,
    now: f64,
    states: &HashMap<u64, PlayerState>,
) {
    history.buf.push_back((now, states.clone()));
    if history.buf.len() > history.cap {
        history.buf.pop_front();
    }
}

/// Лаг‑компенсация попадания
pub fn check_hit_lag_comp(
    history: &VecDeque<(f64, HashMap<u64, PlayerState>)>,
    current: &HashMap<u64, PlayerState>,
    shoot: &ShootEvent,
) -> Option<u64> {
    let states_at_shot = history.iter()
        .min_by(|a, b| (a.0 - shoot.timestamp).abs().partial_cmp(&(b.0 - shoot.timestamp).abs()).unwrap())
        .map(|(_, m)| m)
        .unwrap_or(current);

    let shooter = states_at_shot.get(&shoot.shooter_id)?;
    let shooter_pos = shooter.pos;
    let dir = Vec2::new(shoot.dir.x, shoot.dir.y).normalize_or_zero();

    for (&id, target) in states_at_shot.iter() {
        if id == shoot.shooter_id { continue; }
        let to_target = target.pos - shooter_pos;
        let proj = to_target.project_onto(dir);
        if proj.length() <= MAX_RAY_LEN && to_target.distance(proj) <= HITBOX_RADIUS {
            return Some(id);
        }
    }
    None
}
