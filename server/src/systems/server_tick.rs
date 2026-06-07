use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    combat::in_fov,
    constants::{CH_S2C, PLAYER_SIZE, TICK_DT, VIEW_FOV_HALF_ANGLE, VIEW_RADIUS},
    messages::{NpcSnapshot, PlayerSnapshot, WorldSnapshot, S2C},
};
use protocol::abilities::{tick_abilities, AbilityConfig, AbilityInput};
use protocol::messages::InputState;
use std::collections::HashMap;
use crate::{
    resources::{AppliedSeqs, NpcMode, Npcs, PendingInputs, PlayerStates, Reveals, ServerTickTimer, SnapshotHistory, WallGridRes}, utils::push_history
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
    npcs: Res<Npcs>,
    reveals: Res<Reveals>,
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

    // Рывки, стартовавшие в этот тик — разошлём FX после цикла (источник правды
    // о начале рывка — сервер; клиент по этому событию заводит анимацию ролла).
    let mut dash_events: Vec<(u64, Vec2)> = Vec::new();

    // Тикаем способности и движение ВСЕХ живых игроков (стамина/кулдауны идут
    // даже без ввода — иначе регенерация бы зависала).
    for (&_id, st) in states.0.iter_mut() {
        let input = latest.get(&_id);

        let mut wish = Vec2::ZERO;
        let (mut want_block, mut want_dash) = (false, false);
        // по умолчанию (нет ввода) целимся в текущий угол → разворота не будет
        let mut aim = st.rot;
        if let Some(inp) = input {
            if inp.up { wish.y += 1.; }
            if inp.down { wish.y -= 1.; }
            if inp.left { wish.x -= 1.; }
            if inp.right { wish.x += 1.; }
            want_block = inp.block;
            want_dash = inp.dash;
            aim = inp.rotation; // ЖЕЛАЕМЫЙ угол; модель довернётся плавно
            st.stance = inp.stance.clone();
        }
        st.aim = aim; // куда смотрит курсор (для блока и поля зрения)
        wish = wish.normalize_or_zero();

        let out = tick_abilities(
            &mut st.abilities,
            &AbilityInput {
                move_dir: wish,
                aim,
                want_dash,
                want_block,
            },
            TICK_DT,
            &cfg,
        );
        st.blocking = out.blocking;
        // авторитетный угол = плавно довёрнутый (с учётом заморозки на ударе/рывке)
        st.rot = out.facing;
        if out.did_dash {
            dash_events.push((_id, st.abilities.dash_dir));
        }

        let delta = out.move_dir * out.speed * TICK_DT;
        st.pos = walls.0.slide_circle(st.pos, delta, half);
    }

    // КОЛЛИЗИЯ ИГРОК↔ИГРОК (obstacle): расталкиваем пересекающиеся круги, чтобы
    // не проходить сквозь друг друга. Несколько итераций для устойчивости; сдвиг
    // прогоняем через slide_circle, чтобы не затолкать игрока в стену. Авторитет —
    // сервер; клиент плавно подтянется реконсиляцией/интерполяцией.
    let min_dist = PLAYER_SIZE; // сумма радиусов (по half на каждого)
    let ids: Vec<u64> = states.0.keys().copied().collect();
    for _ in 0..4 {
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let a = states.0[&ids[i]].pos;
                let b = states.0[&ids[j]].pos;
                let d = b - a;
                let dist = d.length();
                if dist >= min_dist {
                    continue;
                }
                // нормаль расталкивания (если центры совпали — берём ось X)
                let n = if dist > 1e-4 { d / dist } else { Vec2::new(1.0, 0.0) };
                let push = n * ((min_dist - dist) * 0.5);
                let na = walls.0.slide_circle(a, -push, half);
                let nb = walls.0.slide_circle(b, push, half);
                states.0.get_mut(&ids[i]).unwrap().pos = na;
                states.0.get_mut(&ids[j]).unwrap().pos = nb;
            }
        }
    }

    // рассылаем FX рывков всем (включая самого дашера — у него ролл заводится по
    // этому же событию, чтобы анимация играла надёжно на каждый реальный рывок)
    if !dash_events.is_empty() {
        let endpoint = server.endpoint_mut();
        for (pid, dir) in dash_events {
            endpoint
                .broadcast_message_on(CH_S2C, S2C::DashFx { player_id: pid, dir })
                .ok();
        }
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
    // Полный набор снапшотов неписей (куллится так же, как игроки).
    let all_npcs: Vec<NpcSnapshot> = npcs
        .0
        .iter()
        .map(|(&id, n)| NpcSnapshot {
            id,
            x: n.pos.x,
            y: n.pos.y,
            facing: n.facing,
            hp: n.hp,
            aggro: n.mode == NpcMode::Chase,
        })
        .collect();

    let r2 = VIEW_RADIUS * VIEW_RADIUS;
    let endpoint = server.endpoint_mut();
    let clients = endpoint.clients(); // owned Vec<ClientId>, чтобы не держать заём
    for viewer in clients {
        let (players, npcs_v): (Vec<PlayerSnapshot>, Vec<NpcSnapshot>) = match states.0.get(&viewer) {
            Some(vst) => {
                let vpos = vst.pos;
                let vaim = vst.aim; // поле зрения вокруг КУРСОРА (а не довёрнутой модели)
                let visible = |target: Vec2| -> bool {
                    if (target - vpos).length_squared() > r2 {
                        return false; // за пределами радиуса
                    }
                    // поле зрения: нельзя видеть за спиной (анти-чит)
                    if !in_fov(vpos, vaim, target, VIEW_FOV_HALF_ANGLE) {
                        return false;
                    }
                    !walls.0.segment_blocked(vpos, target, 0.001) // стена на линии?
                };
                // подсвеченные атакующие (вне поля зрения, но недавно били) —
                // их шлём жертве в обход FOV-куллинга.
                let rev_p = reveals.players.get(&viewer);
                let rev_n = reveals.npcs.get(&viewer);
                let player_revealed = |id: u64| rev_p.map_or(false, |m| m.get(&id).map_or(false, |&t| t > server_time));
                let npc_revealed = |id: u32| rev_n.map_or(false, |m| m.get(&id).map_or(false, |&t| t > server_time));
                let players = all
                    .iter()
                    .filter(|ps| ps.id == viewer || visible(Vec2::new(ps.x, ps.y)) || player_revealed(ps.id))
                    .cloned()
                    .collect();
                let npcs_v = all_npcs
                    .iter()
                    .filter(|ns| visible(Vec2::new(ns.x, ns.y)) || npc_revealed(ns.id))
                    .cloned()
                    .collect();
                (players, npcs_v)
            }
            // зритель ещё не заспавнен — шлём пустой снапшот (только время/seq),
            // чтобы он перешёл в InGame; видимость появится после спавна.
            None => (Vec::new(), Vec::new()),
        };

        let snap = WorldSnapshot {
            players,
            npcs: npcs_v,
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
        // игрок слева от стены, толкается вправо в стену → подъезжает вплотную,
        // но не пробивает стену и не откатывается назад (скольжение/«прижатие»)
        let start = Vec2::new(-r - 1.0, 16.0);
        let p = grid.slide_circle(start, Vec2::new(5.0, 0.0), r);
        assert!(p.x + r <= 1e-3, "не должен пробивать стену: {p:?}");
        assert!(p.x >= start.x - 1e-4, "не должен откатываться назад: {p:?}");
    }
}