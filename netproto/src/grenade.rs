//! Граната/банка на Lightyear: серверная физика полёта (общая `protocol::grenade`)
//! + репликация позиции (интерполяция у клиента). Бросок приходит как
//! `NetInput.throw` (точка-цель); сервер спавнит сущность гранаты, ведёт её физику
//! и при детонации (по пройденному пути / удару о стену / таймеру) наносит урон по
//! площади игрокам и неписям, затем despawn.
//!
//! Стены (AABB) подключатся вместе с картой на Stage 8 — пока список пуст и банка
//! летит по прямой до лимита дальности/таймера.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use protocol::constants::{
    GRENADE_BLAST_RADIUS, GRENADE_MAX_BOUNCES, GRENADE_RADIUS, GRENADE_SPEED, GRENADE_TIMER,
    GRENADE_USAGE_COOLDOWN, TICK_DT, grenade_max_reach,
};
use protocol::grenade::{GrenadePhys, blast_damage, step_grenade};
use serde::{Deserialize, Serialize};

use crate::server_sim::DamageAttribution;
use crate::{Health, NetInput, Npc, Player, Position};

/// Реплицируемый маркер гранаты + владелец (для начисления киллов позже).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Grenade {
    pub owner: PeerId,
}

/// Серверная (НЕ реплицируемая) физика полёта банки.
#[derive(Component)]
pub struct GrenadePhysics {
    pub from: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    pub elapsed: f32,
    pub timer: f32,
    pub traveled: f32,
    pub travel_limit: f32,
    pub bounces: u32,
}

/// Заспавнить гранату (серверный авторитет): реплика всем + интерполяция позиции.
pub fn spawn_grenade(
    commands: &mut Commands,
    owner: PeerId,
    from: Vec2,
    dir: Vec2,
    travel_limit: f32,
) -> Entity {
    commands
        .spawn((
            Grenade { owner },
            GrenadePhysics {
                from,
                dir,
                speed: GRENADE_SPEED,
                elapsed: 0.0,
                timer: GRENADE_TIMER,
                traveled: 0.0,
                travel_limit,
                bounces: 0,
            },
            Position(from),
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
            NetworkVisibility::default(),
        ))
        .id()
}

/// Бросок по вводу: на тике, где у игрока `NetInput.throw = Some(target)` и КД
/// прошёл — спавним банку из позиции игрока в направлении цели (с клампом по
/// дальности). КД на бросок храним per-entity в `Local` (детерминированно тикает).
pub fn spawn_grenades(
    mut commands: Commands,
    q: Query<(Entity, &Player, &Position, &ActionState<NetInput>)>,
    mut cooldowns: Local<HashMap<Entity, f32>>,
) {
    for v in cooldowns.values_mut() {
        *v = (*v - TICK_DT).max(0.0);
    }
    for (e, player, pos, action) in &q {
        let Some(target) = action.0.throw else {
            continue;
        };
        let cd = cooldowns.entry(e).or_insert(0.0);
        if *cd > 0.0 {
            continue;
        }
        let origin = pos.0;
        let to = target - origin;
        let dir = to.normalize_or_zero();
        if dir == Vec2::ZERO {
            continue;
        }
        *cd = GRENADE_USAGE_COOLDOWN as f32;
        let reach = grenade_max_reach();
        let want = to.length().min(reach);
        let spawn_offset = GRENADE_RADIUS + 1.0;
        let from = origin + dir * spawn_offset;
        let travel_limit = (want - spawn_offset).max(0.0);
        let g = spawn_grenade(&mut commands, player.0, from, dir, travel_limit);
        info!("player {e:?} threw grenade {g:?} -> ({:.0},{:.0})", target.x, target.y);
    }
}

/// Физика полёта + детонация: ведём все гранаты, при подрыве наносим урон по
/// площади игрокам/неписям и despawn. Запускать в `FixedUpdate` после движения.
#[allow(clippy::type_complexity)]
pub fn update_grenades(
    mut commands: Commands,
    mut grenades: Query<(Entity, &mut Position, &mut GrenadePhysics, &Grenade), With<Grenade>>,
    mut players: Query<(Entity, &Position, &mut Health), (With<Player>, Without<Npc>, Without<Grenade>)>,
    mut npcs: Query<(&Position, &mut Health), (With<Npc>, Without<Grenade>)>,
    map: Option<Res<crate::map::MapGrids>>,
    mut attribution: ResMut<DamageAttribution>,
    mut fx: ResMut<crate::fx::FxOut>,
) {
    let dt = TICK_DT;
    let walls: &[(Vec2, Vec2)] = map.as_deref().map(|m| m.wall_aabbs.as_slice()).unwrap_or(&[]);

    let mut detonations: Vec<(Vec2, u64)> = Vec::new();
    let mut despawn: Vec<Entity> = Vec::new();

    for (e, mut pos, mut ph, grenade) in &mut grenades {
        let phys = GrenadePhys {
            from: ph.from,
            dir: ph.dir,
            speed: ph.speed,
            elapsed: ph.elapsed,
        };
        let can_bounce = ph.bounces < GRENADE_MAX_BOUNCES;
        let prev = pos.0;
        let res = step_grenade(&phys, dt, walls, can_bounce);

        ph.from = res.state.from;
        ph.dir = res.state.dir;
        ph.speed = res.state.speed;
        ph.elapsed = res.state.elapsed;
        pos.0 = res.pos;
        ph.traveled += (res.pos - prev).length();
        ph.timer -= dt;

        let mut boom = ph.traveled >= ph.travel_limit || ph.timer <= 0.0;
        if res.hit_wall {
            if res.bounced {
                ph.bounces += 1;
            } else {
                boom = true; // отскоки исчерпаны — разбивается о стену
            }
        }
        if boom {
            detonations.push((res.pos, grenade.owner.to_bits()));
            despawn.push(e);
        }
    }

    for (pos, owner_cid) in &detonations {
        info!("grenade detonated at ({:.0},{:.0})", pos.x, pos.y);
        fx.push(crate::messages::Fx::Detonation { pos: *pos });
        for (pe, ppos, mut hp) in &mut players {
            if hp.0 <= 0 {
                continue; // труп взрывом не «доубивается»
            }
            let d = (ppos.0 - *pos).length();
            if d <= GRENADE_BLAST_RADIUS {
                let dmg = blast_damage(d);
                if dmg > 0 {
                    hp.0 -= dmg;
                    attribution.record(pe, *owner_cid);
                    info!("  blast -> player -{dmg} (hp={})", hp.0);
                }
            }
        }
        for (npos, mut hp) in &mut npcs {
            let d = (npos.0 - *pos).length();
            if d <= GRENADE_BLAST_RADIUS {
                let dmg = blast_damage(d);
                if dmg > 0 {
                    hp.0 -= dmg;
                    info!("  blast -> npc -{dmg} (hp={})", hp.0);
                }
            }
        }
    }

    for e in despawn {
        commands.entity(e).despawn();
    }
}
