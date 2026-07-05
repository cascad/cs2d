//! Непись (скелеты/зомби) на Lightyear: серверный авторитетный ИИ + репликация
//! (интерполяция у клиента). Это перенос `server/src/systems/npc.rs` на ECS-
//! сущности и компоненты протокола. Маршруты/стены карты подключатся на этапе
//! интеграции карты (Stage 8); пока непись idle у «дома» и агрит ближайшего
//! игрока в радиусе видимости (без проверки стен), преследует и бьёт.
//!
//! Источник истины боя «игрок → непись» — `Strikes` (заполняется в
//! `server_simulate`): здесь мы применяем удары/станы по неписям. Удар «непись →
//! игрок» наносится прямо отсюда (после движения), как и в старом ИИ.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::*;
use protocol::combat::{block_absorbs, in_melee_sector};
use protocol::constants::{
    BLOCK_ARC_HALF_ANGLE, BLOCK_HIT_STAMINA_COST, MELEE_HALF_ANGLE, NPC_ATTACK_ANIM,
    NPC_ATTACK_COOLDOWN, NPC_ATTACK_DAMAGE, NPC_ATTACK_RANGE, NPC_AUDIO_RADIUS, NPC_CHASE_SPEED,
    NPC_GROWL_INTERVAL, NPC_HIT_DELAY, NPC_HP, NPC_RADIUS, NPC_RESPAWN_TIME, NPC_SIGHT_RADIUS,
    STUN_DURATION, TICK_DT,
};
use protocol::messages::{NpcKind, NpcSoundKind};

use crate::auth::Accounts;
use crate::map::MapGrids;
use crate::messages::{Fx, FxKind};
use crate::server_sim::Strikes;
use crate::{AbilityState, Blocking, Health, Npc, NpcRuntime, Player, Position, Rotation};

/// Сжатие AABB стен при проверке LOS (как в старом серверном куллинге).
const LOS_EPS: f32 = 1.0;

/// Серверное (НЕ реплицируемое) состояние ИИ неписи: таймеры атаки и «дом».
#[derive(Component)]
pub struct NpcBrain {
    pub attack_cd: f32,
    pub attack_anim: f32,
    /// Отложенный удар: урон прилетит цели через столько секунд (середина
    /// клипа атаки, `NPC_HIT_DELAY`) — от замаха можно успеть отойти.
    pub hit_left: f32,
    /// Цель отложенного удара (замах уже начался по этому игроку).
    pub hit_target: Option<Entity>,
    pub home: Vec2,
}

/// Очередь респауна погибших неписей: (тип, точка «дома», осталось сек).
/// Популяция карты постоянна: убитая непись возвращается на свою точку спавна
/// через `NPC_RESPAWN_TIME`.
#[derive(Resource, Default)]
pub struct NpcRespawns(pub Vec<(NpcKind, Vec2, f32)>);

/// Тикает таймеры очереди респауна. Непись возрождается НЕ на «дому» (посреди
/// комнаты, часто за спиной игрока), а в углу карты, самом дальнем от живых
/// игроков, — и оттуда идёт к дому. Появление читаемо: врага видно на подходе.
pub fn respawn_npcs(
    mut queue: ResMut<NpcRespawns>,
    map: Option<Res<MapGrids>>,
    players: Query<(&Position, &Health), (With<Player>, Without<Npc>)>,
    mut commands: Commands,
) {
    let mut i = 0;
    while i < queue.0.len() {
        queue.0[i].2 -= TICK_DT;
        if queue.0[i].2 <= 0.0 {
            let (kind, home, _) = queue.0.remove(i);
            // Угол, максимально удалённый от БЛИЖАЙШЕГО живого игрока (maximin).
            // Без игроков/углов — фолбэк на «дом» (как раньше).
            let spawn_at = map
                .as_deref()
                .filter(|m| !m.npc_corners.is_empty())
                .map(|m| {
                    let alive: Vec<Vec2> = players
                        .iter()
                        .filter(|(_, hp)| hp.0 > 0)
                        .map(|(p, _)| p.0)
                        .collect();
                    *m.npc_corners
                        .iter()
                        .max_by(|a, b| {
                            let da = alive
                                .iter()
                                .map(|p| p.distance_squared(**a))
                                .fold(f32::MAX, f32::min);
                            let db = alive
                                .iter()
                                .map(|p| p.distance_squared(**b))
                                .fold(f32::MAX, f32::min);
                            da.total_cmp(&db)
                        })
                        .unwrap_or(&home)
                })
                .unwrap_or(home);
            spawn_npc_at(&mut commands, kind, spawn_at, home);
            info!(
                "npc respawned: {kind:?} @ угол ({:.0},{:.0}) → дом ({:.0},{:.0})",
                spawn_at.x, spawn_at.y, home.x, home.y
            );
        } else {
            i += 1;
        }
    }
}

/// Заспавнить непись: реплицируется всем (интерполяция), HP/тип — из протокола.
/// «Дом» (куда возвращается без агра) совпадает с точкой спавна.
pub fn spawn_npc(commands: &mut Commands, kind: NpcKind, pos: Vec2) -> Entity {
    spawn_npc_at(commands, kind, pos, pos)
}

/// Как [`spawn_npc`], но позиция появления и «дом» различаются (респавн из угла:
/// появились в углу, идут к дому).
pub fn spawn_npc_at(commands: &mut Commands, kind: NpcKind, pos: Vec2, home: Vec2) -> Entity {
    commands
        .spawn((
            Npc { kind },
            NpcRuntime {
                aggro: false,
                attacking: false,
                stun_left: 0.0,
            },
            NpcBrain {
                attack_cd: 0.0,
                attack_anim: 0.0,
                hit_left: 0.0,
                hit_target: None,
                home,
            },
            Position(pos),
            Rotation(core::f32::consts::FRAC_PI_2),
            Health(NPC_HP),
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
            // Туман войны: непись реплицируется клиенту только в его зоне интереса.
            NetworkVisibility::default(),
        ))
        .id()
}

/// Авторитетный тик неписей: применяем удары игроков (из `Strikes`), затем ИИ
/// (агр/преследование/атака), смерти → despawn. Запускать в `FixedUpdate` ПОСЛЕ
/// `server_simulate` (чтобы `Strikes` уже были заполнены позициями этого тика).
#[allow(clippy::type_complexity)]
pub fn npc_ai(
    strikes: Res<Strikes>,
    mut npcs: Query<
        (
            Entity,
            &Npc,
            &mut Position,
            &mut Rotation,
            &mut Health,
            &mut NpcRuntime,
            &mut NpcBrain,
        ),
        Without<Player>,
    >,
    mut players: Query<
        (
            Entity,
            &Player,
            &Position,
            &Rotation,
            &Blocking,
            &mut AbilityState,
            &mut Health,
        ),
        (With<Player>, Without<Npc>),
    >,
    map: Option<Res<MapGrids>>,
    mut accounts: ResMut<Accounts>,
    mut dirty: ResMut<crate::auth::ScoreboardDirty>,
    mut fx: ResMut<crate::fx::FxOut>,
    mut respawns: ResMut<NpcRespawns>,
    mut commands: Commands,
) {
    let dt = TICK_DT;
    let movement = map.as_deref().map(|m| &m.movement);
    let vision = map.as_deref().map(|m| &m.vision);

    // Снимок ЖИВЫХ игроков (id, позиция) — цели для агра; трупы не интересны.
    let plist: Vec<(Entity, Vec2)> = players
        .iter()
        .filter(|(.., hp)| hp.0 > 0)
        .map(|(e, _, p, ..)| (e, p.0))
        .collect();

    // Урон по игрокам, накопленный за этот тик (применим после цикла неписей).
    let mut player_damage: Vec<(Entity, i32)> = Vec::new();
    // Непись, погибшие в этот тик (despawn после цикла): точка/тип для FX + «дом» для респауна.
    let mut dead: Vec<(Entity, Vec2, NpcKind, Vec2)> = Vec::new();

    for (nid, npc, mut npos, mut nrot, mut nhp, mut nrt, mut brain) in &mut npcs {
        let mut killing_attacker: Option<Entity> = None;
        // 1) удары игроков по этой неписи (без блока — непись не блокирует)
        for s in &strikes.0 {
            if in_melee_sector(
                s.pos,
                s.dir,
                npos.0,
                NPC_RADIUS,
                s.range,
                MELEE_HALF_ANGLE,
                s.half_width,
            ) {
                nhp.0 -= s.damage;
                nrt.aggro = true;
                killing_attacker = Some(s.attacker);
                if s.stun {
                    nrt.stun_left = STUN_DURATION;
                }
            }
        }
        if nhp.0 <= 0 {
            if let Some(attacker) = killing_attacker {
                if let Ok((_, player, ..)) = players.get(attacker) {
                    accounts.0.add_npc_kill(player.0.to_bits());
                    // Без флага счётчик копился только в памяти: таблица очков не
                    // перерассылалась (и аккаунты не сохранялись) до ближайшего
                    // ДРУГОГО события — входа/смерти игрока. На проде это
                    // выглядело как «убитые неписи не считаются».
                    dirty.0 = true;
                }
            }
            dead.push((nid, npos.0, npc.kind, brain.home));
            continue;
        }

        // 2) таймеры
        brain.attack_cd = (brain.attack_cd - dt).max(0.0);
        brain.attack_anim = (brain.attack_anim - dt).max(0.0);

        // оглушение: непись бездействует, замах (вкл. «летящий» урон) обрывается
        if nrt.stun_left > 0.0 {
            nrt.stun_left = (nrt.stun_left - dt).max(0.0);
            brain.attack_anim = 0.0;
            brain.hit_target = None;
            nrt.attacking = false;
            continue;
        }

        // 2б) дозревание отложенного удара: урон в СЕРЕДИНЕ клипа атаки, и только
        // если цель ВСЁ ЕЩЁ в досягаемости (с небольшим запасом) — от замаха
        // можно уйти шагом назад или рывком. Поднятый ЩИТ (фронт жертвы к неписи,
        // хватает стамины) поглощает удар целиком — как в PvP.
        if let Some(target) = brain.hit_target {
            brain.hit_left -= dt;
            if brain.hit_left <= 0.0 {
                brain.hit_target = None;
                let reach = NPC_ATTACK_RANGE * 1.25;
                if let Ok((_, _, ppos, prot, pblk, mut pabil, php)) = players.get_mut(target) {
                    // цель уже мертва (добил кто-то другой) — удар в пустоту
                    if php.0 > 0 && (ppos.0 - npos.0).length_squared() <= reach * reach {
                        let blocked = pblk.0
                            && block_absorbs(ppos.0, prot.0, npos.0, BLOCK_ARC_HALF_ANGLE)
                            && pabil.0.stamina >= BLOCK_HIT_STAMINA_COST;
                        if blocked {
                            pabil.0.stamina =
                                (pabil.0.stamina - BLOCK_HIT_STAMINA_COST).max(0.0);
                            fx.push(Fx::Combat {
                                kind: FxKind::Blocked,
                                pos: ppos.0,
                                dir: (ppos.0 - npos.0).normalize_or_zero(),
                            });
                        } else {
                            player_damage.push((target, NPC_ATTACK_DAMAGE));
                        }
                    }
                }
            }
        }

        // 3) ближайший игрок в радиусе видимости И в прямой видимости (LOS через
        //    сетку обзора — за стеной непись не агрит).
        let mut seen: Option<(Entity, Vec2, f32)> = None;
        for (pid, ppos) in &plist {
            let d2 = (*ppos - npos.0).length_squared();
            if d2 > NPC_SIGHT_RADIUS * NPC_SIGHT_RADIUS {
                continue;
            }
            let blocked = vision.map_or(false, |g| g.segment_blocked(npos.0, *ppos, LOS_EPS));
            if blocked {
                continue;
            }
            if seen.map_or(true, |(_, _, bd)| d2 < bd) {
                seen = Some((*pid, *ppos, d2));
            }
        }

        if let Some((pid, ppos, d2)) = seen {
            nrt.aggro = true;
            let to = ppos - npos.0;
            if to.length_squared() > 1.0 {
                let dir = to.normalize();
                nrot.0 = dir.y.atan2(dir.x);
                let in_range = d2 <= NPC_ATTACK_RANGE * NPC_ATTACK_RANGE;
                if in_range {
                    if brain.attack_cd <= 0.0 {
                        brain.attack_cd = NPC_ATTACK_COOLDOWN;
                        brain.attack_anim = NPC_ATTACK_ANIM;
                        // урон НЕ сразу: дозреет в середине клипа (NPC_HIT_DELAY)
                        brain.hit_left = NPC_HIT_DELAY;
                        brain.hit_target = Some(pid);
                        fx.push(crate::messages::Fx::NpcSound {
                            kind: protocol::messages::NpcSoundKind::Attack,
                            pos: npos.0,
                        });
                    }
                } else {
                    npos.0 = npc_slide(movement, npos.0, dir * NPC_CHASE_SPEED * dt);
                }
            }
        } else {
            nrt.aggro = false;
            // вернуться «домой» (idle-патруль появится с маршрутами карты)
            let to = brain.home - npos.0;
            if to.length() > 2.0 {
                let dir = to.normalize();
                nrot.0 = dir.y.atan2(dir.x);
                npos.0 = npc_slide(movement, npos.0, dir * NPC_CHASE_SPEED.min(38.0) * dt);
            }
        }

        nrt.attacking = brain.attack_anim > 0.0;
    }

    // урон игрокам от неписей (по трупам не добиваем)
    for (pid, amount) in player_damage {
        if let Ok((.., mut hp)) = players.get_mut(pid) {
            if hp.0 > 0 {
                hp.0 -= amount;
                info!("npc hit player {pid:?}: -{amount} hp -> {}", hp.0);
            }
        }
    }

    // смерти неписей → FX смерти + despawn (репликация прекращается, клиент убирает)
    // + постановка в очередь респауна на точку «дома».
    for (nid, npos, kind, home) in dead {
        info!("npc {nid:?} died");
        fx.push(crate::messages::Fx::Death {
            kind: crate::messages::DeathKind::Npc(kind),
            pos: npos,
        });
        respawns.0.push((kind, home, NPC_RESPAWN_TIME));
        commands.entity(nid).despawn();
    }
}

/// Скольжение неписи (радиус `NPC_RADIUS`) вдоль стен; без карты — прямой сдвиг.
fn npc_slide(walls: Option<&protocol::geom::WallGrid>, pos: Vec2, delta: Vec2) -> Vec2 {
    match walls {
        Some(g) => g.slide_circle(pos, delta, NPC_RADIUS),
        None => pos + delta,
    }
}

// ============================================================================
// Амбиентные рыки (порт со старого стека): на каждого игрока изредка выбираем
// случайную слышимую непись в `NPC_AUDIO_RADIUS` (слышно и за стеной) и шлём
// позиционный `NpcSound::Growl`. Клиент делает тише/громче по дистанции — на
// слух понятно, далеко ли невидимая угроза.
// ============================================================================

/// Планировщик рыков: на каждого игрока — момент следующего рыка + xorshift-ГСЧ.
#[derive(Resource)]
pub struct NpcGrowls {
    /// Глобальные часы (сек, монотонно растут) — общая шкала для всех таймеров.
    clock: f32,
    /// client_id → момент (по `clock`), когда можно издать следующий рык.
    next: HashMap<u64, f32>,
    rng: u64,
}

impl Default for NpcGrowls {
    fn default() -> Self {
        Self {
            clock: 0.0,
            next: HashMap::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl NpcGrowls {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.rng | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
    /// Случайное число в [0,1).
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32
    }
}

/// Тик планировщика рыков (FixedUpdate). FX уходит широковещательно — рык,
/// запланированный для одного игрока, услышат и соседи (звук позиционный).
pub fn npc_growls(
    mut sched: ResMut<NpcGrowls>,
    npcs: Query<&Position, (With<Npc>, Without<Player>)>,
    players: Query<(&Player, &Position), (With<Player>, Without<Npc>)>,
    mut fx: ResMut<crate::fx::FxOut>,
) {
    sched.clock += TICK_DT;
    let clock = sched.clock;
    let r2 = NPC_AUDIO_RADIUS * NPC_AUDIO_RADIUS;
    let npos: Vec<Vec2> = npcs.iter().map(|p| p.0).collect();

    let mut alive: Vec<u64> = Vec::new();
    for (player, ppos) in &players {
        let cid = player.0.to_bits();
        alive.push(cid);
        if clock < sched.next.get(&cid).copied().unwrap_or(0.0) {
            continue;
        }
        let audible: Vec<Vec2> = npos
            .iter()
            .copied()
            .filter(|p| (*p - ppos.0).length_squared() <= r2)
            .collect();
        let when = if audible.is_empty() {
            // никого не слышно — перепроверим через секунду
            clock + 1.0
        } else {
            let idx = (sched.next_u64() as usize) % audible.len();
            fx.push(Fx::NpcSound {
                kind: NpcSoundKind::Growl,
                pos: audible[idx],
            });
            // следующий рык — через интервал с джиттером ±40%
            clock + NPC_GROWL_INTERVAL * (0.6 + 0.8 * sched.unit())
        };
        sched.next.insert(cid, when);
    }
    // забываем отключившихся игроков, чтобы карта не пухла
    sched.next.retain(|id, _| alive.contains(id));
}
