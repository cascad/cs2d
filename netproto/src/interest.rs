//! Туман войны через interest management Lightyear. Сущности с компонентом
//! `NetworkVisibility` реплицируются клиенту ТОЛЬКО когда попадают в его зону
//! интереса.
//!
//! Видимость = ближний круг `NEAR_SIGHT_RADIUS` вокруг игрока (всегда, даже за
//! стеной — это проксимити-осведомлённость, чтобы всё вплотную к игроку всегда
//! было видно) ИЛИ дальний конус обзора `VIEW_RADIUS`×FOV по направлению взгляда
//! С прямой видимостью (LOS через сетку обзора карты — режет только дальний
//! конус, это и есть туман войны на расстоянии).
//!
//! Вышли из зоны — сервер шлёт despawn, вернулись — снова spawn.

use bevy::prelude::*;
use lightyear::prelude::*;
use protocol::combat::in_fov;
use protocol::constants::{NEAR_SIGHT_RADIUS, VIEW_FOV_HALF_ANGLE, VIEW_RADIUS};
use protocol::geom::WallGrid;

use crate::map::MapGrids;
use crate::{Player, Position, Rotation};

/// Сжатие AABB стен при LOS-проверке (как в старом серверном куллинге).
const LOS_EPS: f32 = 1.0;

/// Чистое решение о видимости цели `target` из точки наблюдателя `vpos`,
/// смотрящего в направлении `vfacing` (радианы).
///
/// Ближний круг — всегда видно (LOS игнорируется). Дальний конус — видно,
/// только если отрезок наблюдатель→цель не перекрыт стеной.
#[inline]
pub fn entity_visible(
    vpos: Vec2,
    vfacing: f32,
    target: Vec2,
    vision: Option<&WallGrid>,
) -> bool {
    let d2 = (target - vpos).length_squared();
    if d2 <= NEAR_SIGHT_RADIUS * NEAR_SIGHT_RADIUS {
        return true;
    }
    let in_far_cone =
        d2 <= VIEW_RADIUS * VIEW_RADIUS && in_fov(vpos, vfacing, target, VIEW_FOV_HALF_ANGLE);
    if !in_far_cone {
        return false;
    }
    vision.map_or(true, |g| !g.segment_blocked(vpos, target, LOS_EPS))
}

/// Обновляет видимость всех `NetworkVisibility`-сущностей для каждого клиента.
/// `gain_visibility`/`lose_visibility` кэшируются — повторные вызовы идемпотентны.
pub fn update_interest(
    clients: Query<(&ControlledBy, &Position, &Rotation), With<Player>>,
    mut visibles: Query<(&Position, &mut ReplicationState), With<NetworkVisibility>>,
    map: Option<Res<MapGrids>>,
) {
    // sender-entity клиента = его link; позиция и взгляд берём с его игрока.
    let viewers: Vec<(Entity, Vec2, f32)> =
        clients.iter().map(|(c, p, r)| (c.owner, p.0, r.0)).collect();
    if viewers.is_empty() {
        return;
    }
    let vision = map.as_deref().map(|m| &m.vision);

    for (pos, mut state) in &mut visibles {
        for (sender, vpos, vfacing) in &viewers {
            if entity_visible(*vpos, *vfacing, pos.0, vision) {
                state.gain_visibility(*sender);
            } else {
                state.lose_visibility(*sender);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALL_GRID_CELL: f32 = 64.0; // = TILE(32) * 2, как в map.rs

    /// Стена-AABB между игроком и дальней целью (вертикальная полоса на x≈300).
    fn wall_at_x300() -> WallGrid {
        let walls = vec![(Vec2::new(290.0, -400.0), Vec2::new(310.0, 400.0))];
        WallGrid::build(&walls, WALL_GRID_CELL)
    }

    #[test]
    fn near_circle_visible_even_through_wall() {
        let grid = wall_at_x300();
        // Цель в 40 ед. от игрока, между ними проходит стена на x=300 —
        // но ближний круг видит всегда.
        let vpos = Vec2::new(280.0, 0.0);
        let target = Vec2::new(320.0, 0.0);
        assert!((target - vpos).length() < NEAR_SIGHT_RADIUS);
        assert!(entity_visible(vpos, 0.0, target, Some(&grid)));
    }

    #[test]
    fn far_cone_blocked_by_wall() {
        let grid = wall_at_x300();
        // Игрок смотрит вправо (+x). Цель далеко впереди за стеной → не видно.
        let vpos = Vec2::new(0.0, 0.0);
        let target = Vec2::new(500.0, 0.0);
        assert!((target - vpos).length() > NEAR_SIGHT_RADIUS);
        assert!(!entity_visible(vpos, 0.0, target, Some(&grid)));
    }

    #[test]
    fn far_cone_clear_line_visible() {
        // Без стен дальняя цель в конусе обзора видна.
        let empty = WallGrid::build(&[], WALL_GRID_CELL);
        let vpos = Vec2::new(0.0, 0.0);
        let target = Vec2::new(400.0, 0.0);
        assert!((target - vpos).length() > NEAR_SIGHT_RADIUS);
        assert!((target - vpos).length() < VIEW_RADIUS);
        assert!(entity_visible(vpos, 0.0, target, Some(&empty)));
    }

    #[test]
    fn far_cone_behind_player_not_visible() {
        // Цель далеко позади направления взгляда → вне конуса → не видно.
        let empty = WallGrid::build(&[], WALL_GRID_CELL);
        let vpos = Vec2::new(0.0, 0.0);
        let target = Vec2::new(-400.0, 0.0); // позади, игрок смотрит на +x
        assert!(!entity_visible(vpos, 0.0, target, Some(&empty)));
    }

    #[test]
    fn out_of_all_ranges_not_visible() {
        let empty = WallGrid::build(&[], WALL_GRID_CELL);
        let vpos = Vec2::new(0.0, 0.0);
        let target = Vec2::new(VIEW_RADIUS + 500.0, 0.0);
        assert!(!entity_visible(vpos, 0.0, target, Some(&empty)));
    }
}
