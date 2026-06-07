use crate::constants::{HITBOX_RADIUS, MAX_RAY_LEN};
use crate::resources::PlayerState;
use bevy::prelude::*;
use protocol::geom::WallGrid;
use protocol::messages::ShootEvent;
use std::collections::HashMap;

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

// --- ВАША ФУНКЦИЯ С ДОБАВЛЕННЫМ LOS ---
pub fn check_hit_lag_comp(
    history: &std::collections::VecDeque<(f64, std::collections::HashMap<u64, PlayerState>)>,
    _current: &std::collections::HashMap<u64, PlayerState>,
    shoot: &ShootEvent,
    walls: &WallGrid,        // спатиал-сетка стен
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
    let mut interp: std::collections::HashMap<u64, PlayerState> = std::collections::HashMap::new();
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
                    ..Default::default()
                },
            );
        }
    }

    // луч из позиции стрелка (в интерполированном снапе)
    let shooter = interp.get(&shoot.shooter_id)?;
    let dir = shoot.dir.normalize_or_zero();

    for (&id, target) in interp.iter() {
        if id == shoot.shooter_id {
            continue;
        }
        let to_target = target.pos - shooter.pos;
        // скалярная проекция на нормализованный dir
        let proj_len = to_target.dot(dir);
        if proj_len < 0.0 { continue; } // позади
        if proj_len > MAX_RAY_LEN { continue; }

        // ближайшая точка луча к центру цели
        let nearest = shooter.pos + dir * proj_len;

        // радиальное расстояние от центра цели до луча
        if to_target.length() > 0.0 && to_target.distance(dir * proj_len) <= HITBOX_RADIUS {
            // NEW: проверяем видимость до ближайшей точки попадания (а не до центра)
            if los_blocked_by_walls(shooter.pos, nearest, walls) {
                continue; // стена закрывает — не считаем попаданием
            }
            return Some(id);
        }
    }
    None
}

fn los_blocked_by_walls(p0: Vec2, p1: Vec2, walls: &WallGrid) -> bool {
    // тот же зазор, что и раньше; теперь через спатиал-сетку (O ~ длины пути)
    walls.segment_blocked(p0, p1, 0.001)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::messages::{ShootEvent, Stance};
    use std::collections::{HashMap, VecDeque};

    fn ps(pos: Vec2) -> PlayerState {
        PlayerState {
            pos,
            rot: 0.0,
            stance: Stance::Standing,
            hp: 100,
            ..Default::default()
        }
    }

    /// История из двух одинаковых снапшотов (t=0 и t=1) со стрелком id=1 и целью id=2.
    fn history_with(
        shooter: Vec2,
        target: Vec2,
    ) -> VecDeque<(f64, HashMap<u64, PlayerState>)> {
        let mut s = HashMap::new();
        s.insert(1u64, ps(shooter));
        s.insert(2u64, ps(target));
        let mut h = VecDeque::new();
        h.push_back((0.0f64, s.clone()));
        h.push_back((1.0f64, s));
        h
    }

    #[test]
    fn hits_target_in_line_of_fire() {
        let h = history_with(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0));
        let shoot = ShootEvent {
            shooter_id: 1,
            dir: Vec2::new(1.0, 0.0),
            timestamp: 0.5,
        };
        let empty = WallGrid::default();
        assert_eq!(check_hit_lag_comp(&h, &HashMap::new(), &shoot, &empty), Some(2));
    }

    #[test]
    fn wall_blocks_the_shot() {
        let h = history_with(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0));
        let shoot = ShootEvent {
            shooter_id: 1,
            dir: Vec2::new(1.0, 0.0),
            timestamp: 0.5,
        };
        // стена поперёк траектории между стрелком и целью
        let grid = WallGrid::build(&[(Vec2::new(40.0, -50.0), Vec2::new(60.0, 50.0))], 64.0);
        assert_eq!(check_hit_lag_comp(&h, &HashMap::new(), &shoot, &grid), None);
    }

    #[test]
    fn target_behind_shooter_is_not_hit() {
        let h = history_with(Vec2::new(0.0, 0.0), Vec2::new(-100.0, 0.0));
        let shoot = ShootEvent {
            shooter_id: 1,
            dir: Vec2::new(1.0, 0.0),
            timestamp: 0.5,
        };
        let empty = WallGrid::default();
        assert_eq!(check_hit_lag_comp(&h, &HashMap::new(), &shoot, &empty), None);
    }
}
