use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{CH_S2C, PLAYER_SIZE, TICK_DT, VIEW_RADIUS},
    messages::{PlayerSnapshot, WorldSnapshot, S2C},
};
use protocol::abilities::{tick_abilities, AbilityConfig, AbilityInput};
use protocol::messages::InputState;
use std::collections::HashMap;
use crate::{
    resources::{AppliedSeqs, PendingInputs, PlayerStates, ServerTickTimer, SnapshotHistory, WallGridRes}, utils::push_history
};

/// Server tick: applies pending inputs with sliding wall collision, broadcasts snapshot, and records history
pub fn server_tick(
    time: Res<Time>,
    mut timer: ResMut<ServerTickTimer>,
    mut states: ResMut<PlayerStates>,
    mut pending: ResMut<PendingInputs>,
    mut applied: ResMut<AppliedSeqs>,
    mut history: ResMut<SnapshotHistory>,
    mut server: ResMut<QuinnetServer>,
    walls: Res<WallGridRes>, // AABB-стены для точной круговой коллизии движения
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    // Берём последний ввод каждого игрока за тик и очищаем очереди.
    let mut latest: HashMap<u64, InputState> = HashMap::new();
    for (&id, queue) in pending.0.iter_mut() {
        if let Some(input) = queue.back() {
            applied.0.insert(id, input.seq);
            latest.insert(id, input.clone());
        }
        queue.clear();
    }

    let cfg = AbilityConfig::default();
    let half = PLAYER_SIZE * 0.5;

    // Тикаем способности и движение ВСЕХ живых игроков (стамина/кулдауны идут
    // даже без ввода — иначе регенерация бы зависала).
    for (&_id, st) in states.0.iter_mut() {
        let input = latest.get(&_id);

        let mut wish = Vec2::ZERO;
        let (mut want_block, mut want_dash) = (false, false);
        if let Some(inp) = input {
            if inp.up { wish.y += 1.; }
            if inp.down { wish.y -= 1.; }
            if inp.left { wish.x -= 1.; }
            if inp.right { wish.x += 1.; }
            want_block = inp.block;
            want_dash = inp.dash;
            st.rot = inp.rotation;
            st.stance = inp.stance.clone();
        }
        wish = wish.normalize_or_zero();

        let out = tick_abilities(
            &mut st.abilities,
            &AbilityInput {
                move_dir: wish,
                facing: st.rot,
                want_dash,
                want_block,
            },
            TICK_DT,
            &cfg,
        );
        st.blocking = out.blocking;

        let delta = out.move_dir * out.speed * TICK_DT;
        st.pos = walls.0.slide_circle(st.pos, delta, half);
    }

    let server_time = time.elapsed_secs_f64();

    // Полный набор снапшотов всех игроков считаем один раз.
    let all: Vec<PlayerSnapshot> = states
        .0
        .iter()
        .map(|(&id, st)| PlayerSnapshot {
            id,
            x: st.pos.x,
            y: st.pos.y,
            rotation: st.rot,
            stance: st.stance.clone(),
            hp: st.hp,
            stamina: st.abilities.stamina,
            blocking: st.blocking,
        })
        .collect();

    // ТУМАН ВОЙНЫ (серверный куллинг): каждому клиенту шлём ТОЛЬКО тех, кого он
    // реально видит — себя, и других в радиусе VIEW_RADIUS без стены на линии.
    // Полное состояние остаётся на сервере (для хит-рега/лаг-компенсации),
    // куллится лишь то, что уходит по сети → wallhack невозможен в принципе:
    // данных о невидимых игроках на клиенте просто нет.
    let r2 = VIEW_RADIUS * VIEW_RADIUS;
    let endpoint = server.endpoint_mut();
    let clients = endpoint.clients(); // owned Vec<ClientId>, чтобы не держать заём
    for viewer in clients {
        let players: Vec<PlayerSnapshot> = match states.0.get(&viewer) {
            Some(vst) => {
                let vpos = vst.pos;
                all.iter()
                    .filter(|ps| {
                        if ps.id == viewer {
                            return true; // себя видно всегда
                        }
                        let target = Vec2::new(ps.x, ps.y);
                        if (target - vpos).length_squared() > r2 {
                            return false; // за пределами радиуса
                        }
                        !walls.0.segment_blocked(vpos, target, 0.001) // стена на линии?
                    })
                    .cloned()
                    .collect()
            }
            // зритель ещё не заспавнен — шлём пустой снапшот (только время/seq),
            // чтобы он перешёл в InGame; видимость появится после спавна.
            None => Vec::new(),
        };

        let snap = WorldSnapshot {
            players,
            server_time,
            last_input_seq: applied.0.clone(),
        };
        endpoint
            .send_message_on(viewer, CH_S2C, S2C::Snapshot(snap))
            .ok();
    }

    push_history(&mut history, server_time, &states.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::geom::WallGrid;

    // Стена-AABB 32×32 в мировом квадрате [0,32]^2.
    fn one_wall() -> WallGrid {
        WallGrid::build(&[(Vec2::new(0.0, 0.0), Vec2::new(32.0, 32.0))], 64.0)
    }

    #[test]
    fn server_move_free_space() {
        let grid = WallGrid::build(&[], 64.0);
        let r = PLAYER_SIZE * 0.5;
        let p = grid.slide_circle(Vec2::new(500.0, 500.0), Vec2::new(3.0, 4.0), r);
        assert!((p - Vec2::new(503.0, 504.0)).length() < 1e-4);
    }

    #[test]
    fn server_move_blocked_into_wall() {
        let grid = one_wall();
        let r = PLAYER_SIZE * 0.5;
        // игрок слева от стены, толкается вправо в стену → X не меняется
        let start = Vec2::new(-r - 1.0, 16.0);
        let p = grid.slide_circle(start, Vec2::new(5.0, 0.0), r);
        assert!((p.x - start.x).abs() < 1e-4, "движение в стену должно блокироваться: {p:?}");
    }
}