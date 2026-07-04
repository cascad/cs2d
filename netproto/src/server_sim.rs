//! Серверная (авторитетная) симуляция боя: движение + резолв дискретных ударов
//! (melee/stun) этого тика. Переиспользует геометрию `protocol::combat` и правила
//! из старого `damage.rs` (блок гасит фронтальные 180° при достатке стамины; стан
//! не проходит в блок). Резолв — В ТОТ ЖЕ тик, где `tick_abilities` вернул
//! `did_attack/did_stun` (детерминированно, по tick-synced вводу) — без прежних
//! очередей на wall-clock таймере.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::input::native::ActionState;
use protocol::abilities::AbilityConfig;
use protocol::combat::{block_absorbs, in_melee_sector};
use protocol::constants::{
    BLOCK_ARC_HALF_ANGLE, BLOCK_HIT_STAMINA_COST, DASH_CD_GRACE, MELEE_CD_GRACE, MELEE_DAMAGE,
    MELEE_HALF_ANGLE, MELEE_HALF_WIDTH, MELEE_HIT_DELAY, MELEE_RANGE, PLAYER_SIZE,
    RESPAWN_COOLDOWN, STUN_CD_GRACE, STUN_DAMAGE, STUN_HALF_WIDTH, STUN_HIT_DELAY, STUN_RANGE,
    TICK_DT,
};

use crate::auth::{Accounts, ScoreboardDirty};
use crate::fx::FxOut;
use crate::map::MapGrids;
use crate::messages::{DeathKind, Fx, FxKind};
use crate::sim::slide;
use crate::{AbilityState, Blocking, Health, NetInput, Player, Position, Rotation, step_player};

/// HP, с которым игрок возрождается (как в старом стеке).
const PLAYER_RESPAWN_HP: i32 = 100;

/// Серверный маркер «игрок мёртв, ждёт респауна» (клиенту не реплицируется —
/// тот сам прячет модель по HP ≤ 0). Пока висит: ввод игнорируется, игрок не
/// цель для ударов/агра. По истечении `left` — респаун на точке спавна.
#[derive(Component)]
pub struct Dead {
    pub left: f32,
}

/// Общий конфиг способностей для сервера и клиентского предсказания: grace-периоды
/// дают милость к сетевой задержке и снижают мисспредикты КД/стана/рывка.
pub fn server_cfg() -> AbilityConfig {
    AbilityConfig {
        melee_grace: MELEE_CD_GRACE,
        dash_grace: DASH_CD_GRACE,
        stun_grace: STUN_CD_GRACE,
        ..Default::default()
    }
}

/// Удар игрока в «нейтральной» форме — чтобы его мог разрешить и резолвер по
/// неписям (`npc::npc_ai`). В `Strikes` попадают удары, ДОЗРЕВШИЕ в этот тик
/// (см. `PendingStrikes`), читается системами боя ниже по конвейеру FixedUpdate.
#[derive(Clone, Copy)]
pub struct Strike {
    pub attacker: Entity,
    /// client_id атакующего (`PeerId::to_bits`) — для атрибуции килла.
    pub killer_cid: u64,
    pub pos: Vec2,
    pub dir: Vec2,
    pub range: f32,
    pub half_width: f32,
    pub damage: i32,
    /// true → это удар щитом (накладывает стан), false → обычный удар.
    pub stun: bool,
}

/// Удары, ДОЗРЕВШИЕ в этот тик (урон прилетает сейчас). Очищается каждый тик.
#[derive(Resource, Default)]
pub struct Strikes(pub Vec<Strike>);

/// Удары «в полёте»: замах уже начался (FX/анимация ушли клиентам мгновенно),
/// а урон прилетит через `delay_left` — в СЕРЕДИНЕ клипа взмаха
/// (`MELEE_HIT_DELAY`/`STUN_HIT_DELAY`). Из-за задержки от замаха можно уйти;
/// стан или смерть атакующего отменяют его «летящие» удары.
#[derive(Resource, Default)]
pub struct PendingStrikes(pub Vec<(Strike, f32)>);

/// Кто последним нанёс урон игроку в текущем тике (для атрибуции смерти).
/// Ключ — entity жертвы, значение — client_id убийцы (`PeerId::to_bits()`).
#[derive(Resource, Default)]
pub struct DamageAttribution {
    pub last_killer: HashMap<Entity, u64>,
}

impl DamageAttribution {
    pub fn record(&mut self, victim: Entity, killer_cid: u64) {
        self.last_killer.insert(victim, killer_cid);
    }
}

/// Авторитетный тик: шагаем всех игроков по вводу (со скольжением вдоль стен),
/// расталкиваем пересекающиеся пары, затем резолвим удары/станы этого тика.
#[allow(clippy::type_complexity)]
pub fn server_simulate(
    mut q: Query<
        (
            Entity,
            &Player,
            &mut Position,
            &mut Rotation,
            &mut AbilityState,
            &mut Blocking,
            &mut Health,
            Option<&ActionState<NetInput>>,
        ),
        Without<crate::Npc>,
    >,
    mut strikes: ResMut<Strikes>,
    mut pending: ResMut<PendingStrikes>,
    mut attribution: ResMut<DamageAttribution>,
    mut fx: ResMut<FxOut>,
    map: Option<Res<MapGrids>>,
) {
    let cfg = server_cfg();
    strikes.0.clear();
    let walls = map.as_deref().map(|m| &m.movement);

    for (e, player, mut pos, mut rot, mut abil, mut blk, hp, action) in &mut q {
        // мёртвый не двигается и не бьёт — лежит и ждёт респауна
        if hp.0 <= 0 {
            continue;
        }
        let input = action.map(|a| a.0).unwrap_or_default();
        let out = step_player(&mut pos, &mut rot, &mut abil, &mut blk, &input, &cfg, walls);
        // Замах СТАРТУЕТ сейчас (FX/анимация мгновенны), а урон дозреет через
        // hit-delay — в середине клипа. Позиция/направление удара фиксируются на
        // момент замаха: двигаться атакующий всё равно не может (attack_lock).
        if out.did_attack {
            pending.0.push((
                Strike {
                    attacker: e,
                    killer_cid: player.0.to_bits(),
                    pos: pos.0,
                    dir: out.attack_dir,
                    range: MELEE_RANGE,
                    half_width: MELEE_HALF_WIDTH,
                    damage: MELEE_DAMAGE as i32,
                    stun: false,
                },
                MELEE_HIT_DELAY as f32,
            ));
            fx.push(Fx::Combat {
                kind: FxKind::Melee,
                pos: pos.0,
                dir: out.attack_dir,
            });
        }
        if out.did_stun {
            pending.0.push((
                Strike {
                    attacker: e,
                    killer_cid: player.0.to_bits(),
                    pos: pos.0,
                    dir: out.stun_dir,
                    range: STUN_RANGE,
                    half_width: STUN_HALF_WIDTH,
                    damage: STUN_DAMAGE as i32,
                    stun: true,
                },
                STUN_HIT_DELAY as f32,
            ));
            fx.push(Fx::Combat {
                kind: FxKind::Stun,
                pos: pos.0,
                dir: out.stun_dir,
            });
        }
        if out.did_dash {
            fx.push(Fx::Combat {
                kind: FxKind::Dash,
                pos: pos.0,
                dir: Vec2::from_angle(out.facing),
            });
        }
    }

    separate_players(&mut q, walls);

    // Дозревание отложенных ударов: чей delay истёк — резолвится СЕЙЧАС (по
    // позициям целей на этот момент — от замаха можно было уйти).
    pending.0.retain_mut(|(s, delay)| {
        *delay -= TICK_DT;
        if *delay <= 0.0 {
            strikes.0.push(*s);
            false
        } else {
            true
        }
    });

    if strikes.0.is_empty() {
        return;
    }

    let targets: Vec<(Entity, Vec2, f32, bool, f32)> = q
        .iter()
        .filter(|(.., h, _)| h.0 > 0) // по мёртвым не попадаем
        .map(|(e, _, p, r, a, b, _h, _)| (e, p.0, r.0, b.0, a.0.stamina))
        .collect();

    let radius = PLAYER_SIZE * 0.5;
    let mut damage: HashMap<Entity, i32> = HashMap::new();
    let mut stun: Vec<Entity> = Vec::new();
    let mut drain: HashMap<Entity, f32> = HashMap::new();

    for s in &strikes.0 {
        for (te, tpos, trot, tblock, tstam) in &targets {
            if *te == s.attacker {
                continue;
            }
            if !in_melee_sector(s.pos, s.dir, *tpos, radius, s.range, MELEE_HALF_ANGLE, s.half_width)
            {
                continue;
            }
            // Блок проверяется В МОМЕНТ ПОПАДАНИЯ (не замаха): щит, поднятый за
            // время полёта удара, успевает защитить.
            let blocked = *tblock
                && block_absorbs(*tpos, *trot, s.pos, BLOCK_ARC_HALF_ANGLE)
                && *tstam >= BLOCK_HIT_STAMINA_COST;
            if blocked {
                *drain.entry(*te).or_insert(0.0) += BLOCK_HIT_STAMINA_COST;
                // «дзынь» щита: урон поглощён — клиенту нужен звук/маркер блока
                // (по падению HP это не поймать, HP не изменился).
                fx.push(Fx::Combat {
                    kind: FxKind::Blocked,
                    pos: *tpos,
                    dir: s.dir,
                });
                continue;
            }
            *damage.entry(*te).or_insert(0) += s.damage;
            attribution.record(*te, s.killer_cid);
            if s.stun {
                stun.push(*te);
            }
        }
    }

    for (e, _player, _pos, _rot, mut abil, _blk, mut hp, _) in &mut q {
        if let Some(d) = damage.get(&e) {
            if *d > 0 {
                hp.0 -= *d;
                info!("hit {e:?}: -{d} hp -> {}", hp.0);
            }
        }
        if let Some(s) = drain.get(&e) {
            abil.0.stamina = (abil.0.stamina - *s).max(0.0);
        }
        if stun.contains(&e) {
            abil.0.apply_stun(&cfg);
            info!("stun applied to {e:?}");
        }
    }

    // Стан прерывает замах: «летящие» удары оглушённого отменяются (его клип
    // атаки на клиенте тоже обрывается позой стана — визуал и урон совпадают).
    if !stun.is_empty() {
        pending.0.retain(|(s, _)| !stun.contains(&s.attacker));
    }
}

/// Единая точка смертей игроков: вызывается в конце FixedUpdate после NPC/гранат.
/// Записывает килл в `Accounts`, шлёт FX смерти и вешает маркер [`Dead`] с
/// таймером — респаун НЕ мгновенный, тело лежит `RESPAWN_COOLDOWN` секунд
/// (см. [`respawn_players`]).
#[allow(clippy::type_complexity)]
pub fn resolve_player_deaths(
    mut commands: Commands,
    mut q: Query<
        (
            Entity,
            &Player,
            &Position,
            &mut AbilityState,
            &mut Blocking,
            &mut Health,
        ),
        (Without<crate::Npc>, Without<Dead>),
    >,
    mut accounts: ResMut<Accounts>,
    mut dirty: ResMut<ScoreboardDirty>,
    mut attribution: ResMut<DamageAttribution>,
    mut fx: ResMut<FxOut>,
    mut pending: ResMut<PendingStrikes>,
) {
    for (e, player, pos, mut abil, mut blk, mut hp) in q.iter_mut() {
        if hp.0 > 0 {
            continue;
        }
        let victim_cid = player.0.to_bits();
        let killer = attribution.last_killer.get(&e).copied();
        accounts.0.record_player_death(victim_cid, killer);
        dirty.0 = true;

        fx.push(Fx::Death {
            kind: DeathKind::Player,
            pos: pos.0,
        });
        info!("player {e:?} died (killer={killer:?}) -> respawn in {RESPAWN_COOLDOWN}s");
        hp.0 = 0;
        abil.0 = protocol::abilities::Abilities::default();
        blk.0 = false;
        commands.entity(e).insert(Dead {
            left: RESPAWN_COOLDOWN,
        });
        // смерть отменяет «летящие» удары погибшего — мёртвый не доносит урон
        pending.0.retain(|(s, _)| s.attacker != e);
    }
    attribution.last_killer.clear();
}

/// Тикает таймеры [`Dead`] и по истечении возрождает игрока на точке спавна с
/// полным HP и чистыми способностями.
#[allow(clippy::type_complexity)]
pub fn respawn_players(
    mut commands: Commands,
    mut q: Query<
        (
            Entity,
            &mut Dead,
            &mut Position,
            &mut AbilityState,
            &mut Blocking,
            &mut Health,
        ),
        Without<crate::Npc>,
    >,
    map: Option<Res<MapGrids>>,
) {
    let spawn = map
        .as_deref()
        .and_then(|m| m.spawns.first().copied())
        .unwrap_or(Vec2::ZERO);
    for (e, mut dead, mut pos, mut abil, mut blk, mut hp) in &mut q {
        dead.left -= TICK_DT;
        if dead.left > 0.0 {
            continue;
        }
        commands.entity(e).remove::<Dead>();
        hp.0 = PLAYER_RESPAWN_HP;
        pos.0 = spawn;
        abil.0 = protocol::abilities::Abilities::default();
        blk.0 = false;
        info!("player {e:?} respawned @ ({:.0},{:.0})", spawn.x, spawn.y);
    }
}

/// Разталкивание пересекающихся игроков (сумма радиусов = `PLAYER_SIZE`), сдвиг
/// со скольжением вдоль стен. Несколько итераций — для устойчивости в кучах.
#[allow(clippy::type_complexity)]
fn separate_players(
    q: &mut Query<
        (
            Entity,
            &Player,
            &mut Position,
            &mut Rotation,
            &mut AbilityState,
            &mut Blocking,
            &mut Health,
            Option<&ActionState<NetInput>>,
        ),
        Without<crate::Npc>,
    >,
    walls: Option<&protocol::geom::WallGrid>,
) {
    let mut ppos: Vec<(Entity, Vec2)> = q
        .iter()
        .filter(|(.., h, _)| h.0 > 0) // труп не расталкивается
        .map(|(e, _, p, ..)| (e, p.0))
        .collect();
    if ppos.len() < 2 {
        return;
    }
    let min_dist = PLAYER_SIZE;
    for _ in 0..4 {
        for i in 0..ppos.len() {
            for j in (i + 1)..ppos.len() {
                let a = ppos[i].1;
                let b = ppos[j].1;
                let d = b - a;
                let dist = d.length();
                if dist < min_dist {
                    let n = if dist > 1e-4 { d / dist } else { Vec2::new(1.0, 0.0) };
                    let push = n * ((min_dist - dist) * 0.5);
                    ppos[i].1 = slide(walls, a, -push);
                    ppos[j].1 = slide(walls, b, push);
                }
            }
        }
    }
    let moved: HashMap<Entity, Vec2> = ppos.into_iter().collect();
    for (e, _, mut pos, ..) in q.iter_mut() {
        if let Some(p) = moved.get(&e) {
            pos.0 = *p;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightyear::prelude::PeerId;
    use protocol::abilities::Abilities;

    #[test]
    fn resolve_deaths_records_kill_and_respawns() {
        let mut app = App::new();
        app.init_resource::<Accounts>();
        app.init_resource::<ScoreboardDirty>();
        app.init_resource::<DamageAttribution>();
        app.init_resource::<FxOut>();
        app.init_resource::<PendingStrikes>();
        app.add_systems(Update, (resolve_player_deaths, respawn_players).chain());

        let victim_cid = 10u64;
        let killer_cid = 20u64;
        app.world_mut()
            .resource_mut::<Accounts>()
            .0
            .authenticate(victim_cid, "Victim", "pw");
        app.world_mut()
            .resource_mut::<Accounts>()
            .0
            .authenticate(killer_cid, "Killer", "pw");

        let victim = app
            .world_mut()
            .spawn((
                Player(PeerId::Netcode(victim_cid)),
                Position(Vec2::new(100.0, 50.0)),
                Rotation(0.0),
                AbilityState(Abilities::default()),
                Blocking(false),
                Health(0),
            ))
            .id();

        app.world_mut()
            .resource_mut::<DamageAttribution>()
            .record(victim, killer_cid);

        app.update();

        // смерть НЕ мгновенный респаун: тело лежит с HP=0 и маркером Dead
        assert_eq!(app.world().get::<Health>(victim).unwrap().0, 0);
        assert!(app.world().get::<Dead>(victim).is_some());

        // дожидаемся кулдауна респауна (тик таймера = TICK_DT за update)
        let ticks = (RESPAWN_COOLDOWN / TICK_DT).ceil() as usize + 1;
        for _ in 0..ticks {
            app.update();
        }
        assert_eq!(app.world().get::<Health>(victim).unwrap().0, PLAYER_RESPAWN_HP);
        assert!(app.world().get::<Dead>(victim).is_none());

        let snap = app.world().resource::<Accounts>().0.snapshot();
        let killer = snap.iter().find(|e| e.name == "Killer").unwrap();
        let victim_row = snap.iter().find(|e| e.name == "Victim").unwrap();
        assert_eq!(killer.kills, 1);
        assert_eq!(victim_row.deaths, 1);
        assert!(app.world().resource::<ScoreboardDirty>().0);
        assert!(app.world().resource::<FxOut>().0.iter().any(|fx| {
            matches!(fx, Fx::Death { kind: DeathKind::Player, .. })
        }));
        assert!(app.world().resource::<DamageAttribution>().last_killer.is_empty());
    }
}
