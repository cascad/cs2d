use crate::constants::{HITBOX_RADIUS, MAX_RAY_LEN};
use crate::resources::PlayerState;
use protocol::messages::ShootEvent;
use std::collections::{HashMap, VecDeque};

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

pub fn check_hit_lag_comp(
    history: &VecDeque<(f64, HashMap<u64, PlayerState>)>,
    current: &HashMap<u64, PlayerState>,
    shoot: &ShootEvent,
) -> Option<u64> {
    // Находим два снапшота вокруг shoot.timestamp
    let mut prev = None;
    let mut next = None;
    for (t, states) in history {
        if *t <= shoot.timestamp {
            prev = Some((t, states));
        } else if next.is_none() {
            next = Some((t, states));
        }
    }
    let (t0, s0, t1, s1) = match (prev, next) {
        (Some((t0, s0)), Some((t1, s1))) => (*t0, s0, *t1, s1),
        (Some((t0, s0)), None) => (*t0, s0, *t0, s0),
        _ => return None,
    };
    let alpha = ((shoot.timestamp - t0) / (t1 - t0).max(1e-4)).clamp(0.0, 1.0) as f32;

    // интерполируем все позиции
    let mut interp: HashMap<u64, PlayerState> = HashMap::new();
    for (&id, p0) in s0.iter() {
        if let Some(p1) = s1.get(&id) {
            let lerped_pos = p0.pos.lerp(p1.pos, alpha);
            let lerped_rot = p0.rot + (p1.rot - p0.rot) * alpha;
            interp.insert(
                id,
                PlayerState {
                    pos: lerped_pos,
                    rot: lerped_rot,
                    stance: p1.stance.clone(),
                    hp: p1.hp,
                },
            );
        }
    }
    // теперь всё как обычно, но по interp
    let shooter = interp.get(&shoot.shooter_id)?;
    let dir = shoot.dir.normalize_or_zero();
    for (&id, target) in interp.iter() {
        if id == shoot.shooter_id {
            continue;
        }
        let to_target = target.pos - shooter.pos;
        let proj = to_target.project_onto(dir);
        if proj.length() <= MAX_RAY_LEN && to_target.distance(proj) <= HITBOX_RADIUS {
            return Some(id);
        }
    }
    None
}
