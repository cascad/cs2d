//! Детерминированная симуляция игрока — общий шаг для клиента (предсказание) и
//! сервера (авторитет). Переиспользует `protocol::abilities::tick_abilities`,
//! поэтому при одинаковом вводе обе стороны считают идентично (нужно для rollback
//! без рассинхрона).

use crate::{AbilityState, Blocking, NetInput, Position, Rotation};
use bevy::prelude::Vec2;
use protocol::abilities::{AbilityConfig, AbilityInput, AbilityOutput, tick_abilities};
use protocol::constants::{PLAYER_SIZE, TICK_DT};
use protocol::geom::WallGrid;

/// Один тик симуляции одного игрока: прогоняет способности и интегрирует движение
/// со скольжением вдоль стен. Один и тот же код у клиента (предсказание/реплей) и
/// сервера (авторитет) — позиции сходятся, у стен нет «отброса».
///
/// `walls` — сетка коллизий движения; `None` означает «открытое пространство»
/// (используется в простых headless-проверках без карты).
pub fn step_player(
    pos: &mut Position,
    rot: &mut Rotation,
    abil: &mut AbilityState,
    blocking: &mut Blocking,
    input: &NetInput,
    cfg: &AbilityConfig,
    walls: Option<&WallGrid>,
) -> AbilityOutput {
    let mut a = abil.0;
    let inp = AbilityInput {
        move_dir: input.move_dir(),
        aim: input.aim,
        want_dash: input.dash,
        want_block: input.block,
        want_attack: input.attack,
        want_stun: input.stun,
    };
    let out = tick_abilities(&mut a, &inp, TICK_DT, cfg);
    abil.0 = a;

    let delta = out.move_dir.normalize_or_zero() * out.speed * TICK_DT;
    pos.0 = slide(walls, pos.0, delta);
    rot.0 = out.facing;
    blocking.0 = out.blocking;
    out
}

/// Скольжение круга игрока вдоль стен (радиус = `PLAYER_SIZE/2`). Без карты —
/// прямая интеграция.
pub fn slide(walls: Option<&WallGrid>, pos: Vec2, delta: Vec2) -> Vec2 {
    match walls {
        Some(g) => g.slide_circle(pos, delta, PLAYER_SIZE * 0.5),
        None => pos + delta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AbilityState, Blocking, NetInput, Position, Rotation};
    use protocol::abilities::{AbilityConfig, Abilities};

    fn run_input(walls: Option<&WallGrid>, input: NetInput, ticks: usize) -> Vec2 {
        let mut pos = Position(Vec2::ZERO);
        let mut rot = Rotation(0.0);
        let mut abil = AbilityState(Abilities::default());
        let mut blk = Blocking(false);
        let cfg = AbilityConfig::default();
        for _ in 0..ticks {
            step_player(&mut pos, &mut rot, &mut abil, &mut blk, &input, &cfg, walls);
        }
        pos.0
    }

    #[test]
    fn slide_none_is_direct_integration() {
        let p = slide(None, Vec2::new(10.0, 5.0), Vec2::new(3.0, -2.0));
        assert_eq!(p, Vec2::new(13.0, 3.0));
    }

    #[test]
    fn open_space_moves_right() {
        // Без карты игрок свободно едет вправо.
        let input = NetInput {
            right: true,
            ..Default::default()
        };
        let p = run_input(None, input, 30);
        assert!(p.x > 1.0, "должен сдвинуться вправо: {p:?}");
    }

    #[test]
    fn wall_blocks_player_movement() {
        // Стена-стенка справа от старта: игрок (радиус PLAYER_SIZE/2) упирается и
        // НЕ проходит сквозь неё, что бы он ни жал.
        let r = PLAYER_SIZE * 0.5;
        let wall_x = r + 40.0;
        let wall = (
            Vec2::new(wall_x, -200.0),
            Vec2::new(wall_x + 32.0, 200.0),
        );
        let grid = WallGrid::build(&[wall], 64.0);
        let input = NetInput {
            right: true,
            ..Default::default()
        };
        let p = run_input(Some(&grid), input, 200);
        assert!(
            p.x + r <= wall_x + 1e-2,
            "игрок не должен пробивать стену: x={} (стена @ {wall_x})",
            p.x
        );
        assert!(p.x > 0.0, "но всё же должен подъехать вплотную: {p:?}");
    }
}
