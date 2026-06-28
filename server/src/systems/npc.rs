//! НЕПИСИ (скелеты): автоспавн, патруль по маршрутам карты, агр на игрока по
//! линии видимости, преследование с «памятью» и атака; смерть → труп на клиенте.
//! ИИ полностью серверный (авторитет); клиент только рисует снапшоты.

use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    constants::{
        CH_S2C, NPC_ATTACK_ANIM, NPC_ATTACK_COOLDOWN, NPC_ATTACK_DAMAGE, NPC_ATTACK_RANGE,
        NPC_CHASE_SPEED, NPC_GIVEUP_TIME, NPC_HP, NPC_RADIUS, NPC_SIGHT_RADIUS, NPC_SPEED,
        PLAYER_SIZE,
    },
    maps,
    messages::{NpcKind, S2C},
};

use crate::{
    events::{DamageEvent, NpcDamageEvent},
    resources::{
        Accounts, Npc, NpcIdCounter, NpcMode, NpcRespawnTimer, NpcRoutes, NpcSpawnPoints, Npcs,
        PlayerStates, WallGridRes,
    },
    systems::level_fixed::TILE,
};
use std::collections::HashSet;

/// Желаемая популяция скелетов на карте (поддерживается автоспавном).
const NPC_TARGET_POP: usize = 5;
/// Порог «прибытия» к точке маршрута (мир. ед.).
const ARRIVE_EPS: f32 = 18.0;

/// Стартовая инициализация: грузим маршруты/спавны с карты и заселяем подземелье.
pub fn setup_npcs(
    mut commands: Commands,
    mut id_counter: ResMut<NpcIdCounter>,
    mut npcs: ResMut<Npcs>,
) {
    let routes = maps::patrol_routes(TILE);
    let spawns = maps::npc_spawns(TILE);

    // заселяем РОВНО NPC_TARGET_POP скелетов, разнося по разным маршрутам
    // (по началу маршрута), пока не наберём нужное число.
    'fill: for &wp in &[0usize, 1usize] {
        for (ri, route) in routes.iter().enumerate() {
            if npcs.0.len() >= NPC_TARGET_POP {
                break 'fill;
            }
            if let Some(&pos) = route.get(wp) {
                spawn_npc(&mut npcs, &mut id_counter, ri, wp, pos);
            }
        }
    }

    commands.insert_resource(NpcRoutes(routes));
    commands.insert_resource(NpcSpawnPoints(spawns));
}

fn spawn_npc(npcs: &mut Npcs, id_counter: &mut NpcIdCounter, route: usize, wp: usize, pos: Vec2) {
    id_counter.0 += 1;
    let id = id_counter.0;
    // Чередуем типы по чётности id → на карте всегда смесь скелетов и зомби.
    let kind = if id % 2 == 0 {
        NpcKind::Zombie
    } else {
        NpcKind::Skeleton
    };
    let facing = std::f32::consts::FRAC_PI_2; // смотрит «вниз» по умолчанию
    npcs.0.insert(
        id,
        Npc {
            pos,
            facing,
            hp: NPC_HP,
            kind,
            mode: NpcMode::Wander,
            route,
            wp,
            wp_dir: 1,
            target_pos: pos,
            lost_timer: 0.0,
            attack_cd: 0.0,
            attack_anim: 0.0,
            home: pos,
        },
    );
}

/// Тик ИИ неписей: урон по ним, состояния, движение, атака, автоспавн.
#[allow(clippy::too_many_arguments)]
pub fn npc_ai(
    time: Res<Time>,
    mut npcs: ResMut<Npcs>,
    players: Res<PlayerStates>,
    routes: Res<NpcRoutes>,
    walls: Res<WallGridRes>,
    mut id_counter: ResMut<NpcIdCounter>,
    mut respawn: ResMut<NpcRespawnTimer>,
    mut npc_dmg: MessageReader<NpcDamageEvent>,
    mut dmg: MessageWriter<DamageEvent>,
    mut accounts: ResMut<Accounts>,
    mut server: ResMut<QuinnetServer>,
) {
    let dt = time.delta_secs();

    // 1) урон по неписям (от игроков) + смерти. Храним и УБИЙЦУ (source), чтобы
    // начислить ему «убитую непись». Гард `dead` не даёт задвоить смерть/килл,
    // если по уже мёртвой непись за тот же тик прилетело ещё одно событие.
    let mut deaths: Vec<(u32, Vec2, f32, NpcKind, Option<u64>)> = Vec::new();
    let mut dead: HashSet<u32> = HashSet::new();
    for ev in npc_dmg.read() {
        if dead.contains(&ev.target) {
            continue;
        }
        if let Some(n) = npcs.0.get_mut(&ev.target) {
            n.hp -= ev.amount;
            // агр при получении урона (даже если бил со спины)
            n.mode = NpcMode::Chase;
            n.lost_timer = 0.0;
            if n.hp <= 0 {
                dead.insert(ev.target);
                deaths.push((ev.target, n.pos, n.facing, n.kind, ev.source));
            }
        }
    }
    let mut scoreboard_dirty = false;
    for (id, _, _, _, killer) in &deaths {
        npcs.0.remove(id);
        if let Some(src) = killer {
            accounts.0.add_npc_kill(*src);
            scoreboard_dirty = true;
        }
    }

    // 2) живые игроки (id, позиция) для проверки видимости
    let plist: Vec<(u64, Vec2)> = players.0.iter().map(|(&id, s)| (id, s.pos)).collect();

    for (&nid, n) in npcs.0.iter_mut() {
        n.attack_cd = (n.attack_cd - dt).max(0.0);
        n.attack_anim = (n.attack_anim - dt).max(0.0);

        // ближайший игрок в радиусе и без стены на линии
        let mut seen: Option<(u64, Vec2, f32)> = None;
        for (pid, ppos) in &plist {
            let d2 = (*ppos - n.pos).length_squared();
            if d2 <= NPC_SIGHT_RADIUS * NPC_SIGHT_RADIUS
                && !walls.0.segment_blocked(n.pos, *ppos, 0.001)
            {
                if seen.map_or(true, |(_, _, bd)| d2 < bd) {
                    seen = Some((*pid, *ppos, d2));
                }
            }
        }

        // выбираем цель движения и скорость по режиму
        let (goal, speed, stop_to_attack) = if let Some((pid, ppos, d2)) = seen {
            n.mode = NpcMode::Chase;
            n.target_pos = ppos;
            n.lost_timer = 0.0;
            let in_range = d2 <= NPC_ATTACK_RANGE * NPC_ATTACK_RANGE;
            if in_range && n.attack_cd <= 0.0 {
                n.attack_cd = NPC_ATTACK_COOLDOWN;
                n.attack_anim = NPC_ATTACK_ANIM;
                dmg.write(DamageEvent {
                    target: pid,
                    amount: NPC_ATTACK_DAMAGE,
                    source: None,
                    source_pos: Some(n.pos),
                    npc_source: Some(nid),
                });
            }
            (ppos, NPC_CHASE_SPEED, in_range)
        } else {
            match n.mode {
                NpcMode::Chase => {
                    n.lost_timer += dt;
                    if n.lost_timer >= NPC_GIVEUP_TIME {
                        n.mode = NpcMode::Return;
                    }
                    (n.target_pos, NPC_CHASE_SPEED, false)
                }
                NpcMode::Return => {
                    // вернуться к ближайшей точке маршрута, затем бродить
                    if let Some(route) = routes.0.get(n.route) {
                        if let Some((idx, _)) = nearest_waypoint(route, n.pos) {
                            n.wp = idx;
                        }
                    }
                    n.mode = NpcMode::Wander;
                    (n.home, NPC_SPEED, false)
                }
                NpcMode::Wander => {
                    let goal = routes
                        .0
                        .get(n.route)
                        .and_then(|r| r.get(n.wp).copied())
                        .unwrap_or(n.home);
                    // дошли до точки — следующая (ping-pong)
                    if (goal - n.pos).length() <= ARRIVE_EPS {
                        if let Some(route) = routes.0.get(n.route) {
                            advance_waypoint(n, route.len());
                        }
                    }
                    (goal, NPC_SPEED, false)
                }
            }
        };

        // движение к цели (если не стоим в атаке)
        let to = goal - n.pos;
        if to.length_squared() > 1.0 {
            let dir = to.normalize();
            n.facing = dir.y.atan2(dir.x);
            if !stop_to_attack {
                // монотонная мировая скорость, как у игроков — одинаково во все
                // стороны (без выравнивания под экран).
                let delta = dir * speed * dt;
                n.pos = walls.0.slide_circle(n.pos, delta, NPC_RADIUS);
            }
        }

        // не даём скелету проходить СКВОЗЬ игрока и оказываться сбоку/за спиной
        // (иначе блок «пробивается»): выталкиваем из личного радиуса игрока.
        // Толкаем ЧЕРЕЗ slide_circle — чтобы НИКОГДА не продавить скелета в стену
        // (иначе он бил бы «сквозь стену» и застревал).
        const PLAYER_HALF: f32 = PLAYER_SIZE * 0.5;
        let min_sep = NPC_RADIUS + PLAYER_HALF;
        for (_, ppos) in &plist {
            let off = n.pos - *ppos;
            let d = off.length();
            if d > 1e-3 && d < min_sep {
                let push = off / d * (min_sep - d);
                n.pos = walls.0.slide_circle(n.pos, push, NPC_RADIUS);
            }
        }
    }

    // 2.5) NPC↔NPC: скелеты не проходят сквозь друг друга (иначе при наложении не
    // видно, что их несколько). Разруливаем попарно, выталкивая каждого на
    // половину перекрытия. Толкаем ЧЕРЕЗ slide_circle, чтобы не продавить в стену.
    let npc_min_sep = NPC_RADIUS * 2.0;
    let ids: Vec<u32> = npcs.0.keys().copied().collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let pi = npcs.0[&ids[i]].pos;
            let pj = npcs.0[&ids[j]].pos;
            let off = pi - pj;
            let d = off.length();
            // совпали почти точка-в-точку → растолкнём по детерминированной оси
            let push = if d > 1e-3 {
                off / d * ((npc_min_sep - d) * 0.5)
            } else {
                Vec2::new(npc_min_sep * 0.5, 0.0)
            };
            if d >= npc_min_sep {
                continue;
            }
            if let Some(n) = npcs.0.get_mut(&ids[i]) {
                n.pos = walls.0.slide_circle(n.pos, push, NPC_RADIUS);
            }
            if let Some(n) = npcs.0.get_mut(&ids[j]) {
                n.pos = walls.0.slide_circle(n.pos, -push, NPC_RADIUS);
            }
        }
    }

    // 3) рассылаем смерти (труп/анимация смерти на клиенте) + таблицу очков
    if !deaths.is_empty() {
        let endpoint = server.endpoint_mut();
        for (id, pos, facing, kind, _killer) in deaths {
            endpoint
                .broadcast_message_on(
                    CH_S2C,
                    S2C::NpcDied { id, x: pos.x, y: pos.y, facing, kind },
                )
                .ok();
        }
        if scoreboard_dirty {
            endpoint
                .broadcast_message_on(CH_S2C, S2C::Scoreboard(accounts.0.snapshot()))
                .ok();
        }
    }

    // 4) автоспавн: держим ПОСТОЯННОЕ число скелетов — пополняем всех недостающих
    if respawn.0.tick(time.delta()).just_finished() && !routes.0.is_empty() {
        while npcs.0.len() < NPC_TARGET_POP {
            let ri = (id_counter.0 as usize) % routes.0.len();
            let route = &routes.0[ri];
            if route.is_empty() {
                break;
            }
            let pos = route[0];
            spawn_npc(&mut npcs, &mut id_counter, ri, 0, pos);
        }
    }
}

/// Следующая путевая точка с разворотом на концах (ping-pong).
fn advance_waypoint(n: &mut Npc, len: usize) {
    if len <= 1 {
        return;
    }
    let next = n.wp as i32 + n.wp_dir;
    if next < 0 {
        n.wp_dir = 1;
        n.wp = 1.min(len - 1);
    } else if next as usize >= len {
        n.wp_dir = -1;
        n.wp = len.saturating_sub(2);
    } else {
        n.wp = next as usize;
    }
}

/// Индекс и дистанция² ближайшей точки маршрута к позиции.
fn nearest_waypoint(route: &[Vec2], pos: Vec2) -> Option<(usize, f32)> {
    route
        .iter()
        .enumerate()
        .map(|(i, w)| (i, (*w - pos).length_squared()))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
}
