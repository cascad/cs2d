//! Клиентская часть НЕПИСЕЙ (скелеты и зомби): загрузка покадровых спрайтов
//! (8 направл. × 8 кадров отдельными PNG), спавн/обновление по снапшоту,
//! интерполяция позиции из буфера, покадровая анимация ходьбы, полоска HP и труп
//! (анимация смерти) по событию `NpcDied`. ИИ/логика общие; тип неписи (`NpcKind`)
//! задаёт лишь набор спрайтов и размер/якорь модели. Источник правды — сервер.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::components::{Facing, NpcAnim, NpcHpFill, NpcKindC, NpcMarker};
use crate::render::{depth_z, layers, world_to_screen, world_to_translation, RenderLayer, WorldPos};
use crate::systems::melee::{make_iso_circle_mesh, MeleeArc, MELEE_ARC_TTL, MELEE_DECAL_LAYER};
use crate::systems::utils::hp_color;
use protocol::constants::{NPC_ATTACK_ANIM, NPC_ATTACK_RANGE, NPC_HP};
use protocol::messages::NpcKind;

/// Папки направлений в порядке экранных секторов: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
const DIR_NAMES: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
const NPC_FRAMES: usize = 8;
const NPC_DIRS: usize = 8;

/// Длительность кадра зацикленных клипов неписи (ходьба/покой), сек.
const NPC_LOOP_FRAME_TIME: f32 = 0.1;

/// Высота полоски HP над неписью (экранные px над точкой ног).
const NPC_HP_Y: f32 = 75.0;

/// Полоска HP неписи.
const NPC_HP_W: f32 = 40.0;
const NPC_HP_H: f32 = 5.0;

/// Визуальные параметры конкретного типа неписи: папка ассетов, экранный размер
/// квадрата спрайта, вертикальный якорь (доля кадра: где «ноги» относительно
/// центра кадра) и цветовой тинт (обычный / в агре).
struct NpcVisual {
    folder: &'static str,
    px: f32,
    anchor_y: f32,
    base: Color,
    aggro: Color,
}

/// Калибровка под конкретные ассеты (вымерено по альфа-боксу фигур):
/// - игрок (рыцарь): кадр 128, фигура ~66px → на экране ~78px при `ISO_ACTOR_PX`=150.
/// - скелет: кадр 256, фигура ~40px → большой квадрат 440, якорь у центра; арт
///   тёмный → подсвечиваем.
/// - зомби: кадр 128, фигура ~46px по высоте, ноги на ~y=82 (на 18px ниже центра).
///   Чтобы зомби был РОВНО с игрока: px = ISO_ACTOR_PX·(66/46) ≈ 220; якорь
///   −18/128 ≈ −0.14, чтобы ноги стояли на тайле.
fn npc_visual(kind: NpcKind) -> NpcVisual {
    match kind {
        NpcKind::Skeleton => NpcVisual {
            folder: "skeleton",
            px: 440.0,
            anchor_y: -0.04,
            base: Color::srgb(1.6, 1.6, 1.65),
            aggro: Color::srgb(1.7, 1.35, 1.3),
        },
        NpcKind::Zombie => NpcVisual {
            folder: "zombie",
            px: 220.0,
            anchor_y: -0.14,
            base: Color::srgb(1.15, 1.15, 1.18),
            aggro: Color::srgb(1.45, 1.05, 1.0),
        },
    }
}

/// Набор кадров одного типа неписи, индексация [направление][кадр]. `walk`/`death`
/// есть у всех; `idle`/`attack` — опциональны (есть у зомби, у скелета в ассетах
/// этих анимаций нет, поэтому `None`).
#[derive(Clone)]
pub struct AnimSet {
    pub walk: Vec<[Handle<Image>; NPC_FRAMES]>,
    pub death: Vec<[Handle<Image>; NPC_FRAMES]>,
    pub idle: Option<Vec<[Handle<Image>; NPC_FRAMES]>>,
    pub attack: Option<Vec<[Handle<Image>; NPC_FRAMES]>>,
}

/// Анимации всех типов неписей (грузятся один раз на старте).
#[derive(Resource, Clone)]
pub struct NpcAnims {
    pub skeleton: AnimSet,
    pub zombie: AnimSet,
}

impl NpcAnims {
    pub fn get(&self, kind: NpcKind) -> &AnimSet {
        match kind {
            NpcKind::Skeleton => &self.skeleton,
            NpcKind::Zombie => &self.zombie,
        }
    }
}

/// id неписи → (hp, aggro, attacking) из реплики — для полоски HP и анимации атаки.
#[derive(Resource, Default)]
pub struct NpcInfo(pub HashMap<u32, (i32, bool, bool)>);

/// id неписи → остаток оглушения (сек) из реплики.
#[derive(Resource, Default)]
pub struct NpcStun(pub HashMap<u32, f32>);

/// Радиус привязки FX-события к ближайшей неписи на экране.
pub const NPC_FX_MATCH_RADIUS: f32 = 96.0;

/// Запускает локальный клип атаки (не ждём `NpcRuntime.attacking` с сервера).
pub fn trigger_npc_attack(anim: &mut NpcAnim) {
    anim.attack_left = NPC_ATTACK_ANIM;
    anim.frame = 0;
    anim.timer.reset();
}

/// Ближайшая непись к точке FX (атака/смерть) с её направлением взгляда.
pub fn nearest_npc(
    pos: Vec2,
    q: &Query<(Entity, &WorldPos, &Facing), With<NpcMarker>>,
) -> Option<(Entity, f32)> {
    let r2 = NPC_FX_MATCH_RADIUS * NPC_FX_MATCH_RADIUS;
    q.iter()
        .filter(|(_, wp, _)| wp.0.distance_squared(pos) <= r2)
        .min_by(|(_, a, _), (_, b, _)| {
            a.0
                .distance_squared(pos)
                .partial_cmp(&b.0.distance_squared(pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(e, _, f)| (e, f.0))
}

#[inline]
pub fn npc_dir(facing_world: f32) -> usize {
    let s = world_to_screen(Vec2::new(facing_world.cos(), facing_world.sin()));
    let deg = s.y.atan2(s.x).to_degrees().rem_euclid(360.0);
    ((deg / 45.0).round() as i32).rem_euclid(8) as usize
}

/// Загрузка всех кадров неписей (скелет + зомби) один раз на старте.
pub fn setup_npc_anims(mut commands: Commands, asset_server: Res<AssetServer>) {
    let load_anim = |folder: &str, name: &str| -> Vec<[Handle<Image>; NPC_FRAMES]> {
        (0..NPC_DIRS)
            .map(|d| {
                std::array::from_fn(|f| {
                    asset_server.load(format!("{}/{}/{}/{}.png", folder, name, DIR_NAMES[d], f))
                })
            })
            .collect()
    };
    // Скелет: в ассетах есть только ходьба и смерть.
    let skeleton = AnimSet {
        walk: load_anim("skeleton", "walk"),
        death: load_anim("skeleton", "death"),
        idle: None,
        attack: None,
    };
    // Зомби: задействуем всё, что есть — простой, ходьба, атака, смерть.
    let zombie = AnimSet {
        walk: load_anim("zombie", "walk"),
        death: load_anim("zombie", "death"),
        idle: Some(load_anim("zombie", "idle")),
        attack: Some(load_anim("zombie", "attack")),
    };
    commands.insert_resource(NpcAnims { skeleton, zombie });
}

/// Экранный размер и якорь спрайта неписи (для моста lynet).
pub fn npc_sprite_layout(kind: NpcKind) -> (f32, f32) {
    let v = npc_visual(kind);
    (v.px, v.anchor_y)
}

/// Покадровая анимация ходьбы неписи + подсветка агра.
pub fn animate_skeletons(
    time: Res<Time>,
    anims: Option<Res<NpcAnims>>,
    info: Res<NpcInfo>,
    mut q: Query<(
        &WorldPos,
        &Facing,
        &NpcKindC,
        &mut NpcAnim,
        &mut Sprite,
        &NpcMarker,
    )>,
) {
    let Some(anims) = anims else { return };
    let dt = time.delta_secs();
    // порог «идёт» по сглаженной СКОРОСТИ (мир. ед./сек): NPC_SPEED=38, поэтому
    // 12 отсекает сетевой шум, но ловит даже медленный патруль.
    const NPC_WALK_EPS_SPEED: f32 = 12.0;
    // время сглаживания скорости (сек): гасит дребезг walk↔idle от снапшотов.
    const MOVE_EMA_TAU: f32 = 0.12;
    for (wp, facing, kind, mut anim, mut sprite, marker) in q.iter_mut() {
        let step = (wp.0 - anim.prev).length();
        anim.prev = wp.0;
        if dt > 0.0 {
            let k = (dt / MOVE_EMA_TAU).clamp(0.0, 1.0);
            anim.move_ema = anim.move_ema * (1.0 - k) + (step / dt) * k;
        }
        let moving = anim.move_ema > NPC_WALK_EPS_SPEED;

        let (aggro, attacking) = info
            .0
            .get(&marker.0)
            .map(|(_, a, atk)| (*a, *atk))
            .unwrap_or((false, false));

        // фронт реплики — запасной триггер, основной — FX NpcSound::Attack
        if attacking {
            anim.attack_left = anim.attack_left.max(NPC_ATTACK_ANIM * 0.35);
        }

        let vis = npc_visual(kind.0);
        let dir = npc_dir(facing.0);
        let set = anims.get(kind.0);

        // (кадры, крутить ли таймер, одноразовый ли клип — без зацикливания)
        let (frames, advancing, oneshot) = if anim.attack_left > 0.0 {
            anim.attack_left = (anim.attack_left - dt).max(0.0);
            if let Some(atk) = set.attack.as_ref() {
                (atk, true, true)
            } else {
                // у скелета нет клипа атаки — короткий проход кадров ходьбы
                (&set.walk, true, true)
            }
        } else if moving {
            (&set.walk, true, false)
        } else if let Some(idle) = set.idle.as_ref() {
            (idle, true, false)
        } else {
            // у скелета нет покоя — стоим на кадре 0 ходьбы
            (&set.walk, false, false)
        };

        // Тайминг кадров: клип АТАКИ укладывается РОВНО в окно NPC_ATTACK_ANIM
        // (все 8 кадров за 0.7с — замах виден целиком, а середина клипа совпадает
        // с моментом урона NPC_HIT_DELAY на сервере); ходьба/покой — LOOP-частота.
        let want_secs = if oneshot {
            NPC_ATTACK_ANIM / NPC_FRAMES as f32
        } else {
            NPC_LOOP_FRAME_TIME
        };
        if (anim.timer.duration().as_secs_f32() - want_secs).abs() > 1e-4 {
            anim.timer
                .set_duration(std::time::Duration::from_secs_f32(want_secs));
        }

        if advancing {
            if anim.timer.tick(time.delta()).just_finished() {
                anim.frame = if oneshot {
                    (anim.frame + 1).min(NPC_FRAMES - 1)
                } else {
                    (anim.frame + 1) % NPC_FRAMES
                };
            }
        } else {
            anim.frame = 0;
        }

        sprite.image = frames[dir][anim.frame.min(NPC_FRAMES - 1)].clone();
        sprite.color = if aggro { vis.aggro } else { vis.base };
    }
}

/// Мгновенная атака по серверному FX (не ждём `NpcRuntime.attacking`).
pub fn apply_npc_sound_anims(
    mut ev: MessageReader<crate::events::NpcSoundEvent>,
    q_npcs: Query<(Entity, &WorldPos, &Facing), With<NpcMarker>>,
    mut q_anim: Query<&mut NpcAnim>,
) {
    use protocol::messages::NpcSoundKind;
    for ev in ev.read() {
        if !matches!(ev.kind, NpcSoundKind::Attack) {
            continue;
        }
        let Some((entity, _)) = nearest_npc(ev.pos, &q_npcs) else {
            continue;
        };
        if let Ok(mut anim) = q_anim.get_mut(entity) {
            trigger_npc_attack(&mut anim);
        }
    }
}

/// Обновляет ширину полоски HP скелета из `NpcInfo`.
pub fn update_npc_hp_bars(
    info: Res<NpcInfo>,
    mut q: Query<(&NpcHpFill, &mut Sprite, &mut Transform)>,
) {
    for (fill, mut sprite, mut tf) in q.iter_mut() {
        let hp = info.0.get(&fill.id).map(|(h, _, _)| *h).unwrap_or(0);
        let frac = (hp as f32 / NPC_HP as f32).clamp(0.0, 1.0);
        let w = fill.full_w * frac;
        sprite.custom_size = Some(Vec2::new(w, NPC_HP_H));
        sprite.color = hp_color(frac);
        tf.translation.x = -fill.full_w * 0.5;
    }
}

/// Спавн трупа неписи (анимация смерти, замирает на последнем кадре). Возвращает
/// сущность, чтобы зарегистрировать её в общем лимите трупов (`Corpses`).
pub fn spawn_npc_corpse(
    commands: &mut Commands,
    anims: &NpcAnims,
    kind: NpcKind,
    pos: Vec2,
    facing: f32,
) -> Entity {
    let vis = npc_visual(kind);
    let dir = npc_dir(facing);
    commands
        .spawn((
            Sprite {
                image: anims.get(kind).death[dir][0].clone(),
                color: vis.base,
                custom_size: Some(Vec2::splat(vis.px)),
                ..default()
            },
            bevy::sprite::Anchor(Vec2::new(0.0, vis.anchor_y)),
            Transform::from_translation(world_to_translation(pos, layers::CORPSE)),
            GlobalTransform::default(),
            WorldPos(pos),
            RenderLayer(layers::CORPSE),
            SkeletonCorpse {
                kind,
                dir,
                frame: 0,
                anim: Timer::from_seconds(1.0 / 12.0, TimerMode::Repeating),
            },
        ))
        .id()
}

/// Труп неписи: проигрывает анимацию смерти один раз и держит последний кадр.
/// НЕ исчезает по таймеру — лежит, пока не вытеснен лимитом трупов.
#[derive(Component)]
pub struct SkeletonCorpse {
    pub kind: NpcKind,
    pub dir: usize,
    pub frame: usize,
    pub anim: Timer,
}

/// Прогон анимации трупа неписи (до последнего кадра, затем удержание).
pub fn animate_skeleton_corpses(
    time: Res<Time>,
    anims: Option<Res<NpcAnims>>,
    mut q: Query<(&mut SkeletonCorpse, &mut Sprite)>,
) {
    let Some(anims) = anims else { return };
    let dt = time.delta();
    for (mut corpse, mut sprite) in q.iter_mut() {
        if corpse.frame < NPC_FRAMES - 1 && corpse.anim.tick(dt).just_finished() {
            corpse.frame += 1;
        }
        sprite.image = anims.get(corpse.kind).death[corpse.dir][corpse.frame].clone();
    }
}

/// Рисуем на полу круг атаки неписи (сервер бьёт по радиусу `NPC_ATTACK_RANGE`
/// вокруг центра, без направления). Спавним РОВНО раз на удар: по фронту
/// `attacking` false→true из снапшота.
pub fn spawn_npc_attack_decals(
    q: Query<(&WorldPos, &NpcMarker)>,
    info: Res<NpcInfo>,
    mut prev: Local<HashMap<u32, bool>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    let mut alive: Vec<u32> = Vec::new();
    for (wp, marker) in q.iter() {
        alive.push(marker.0);
        let attacking = info.0.get(&marker.0).map(|(_, _, a)| *a).unwrap_or(false);
        let was = prev.get(&marker.0).copied().unwrap_or(false);
        prev.insert(marker.0, attacking);
        if !(attacking && !was) {
            continue;
        }
        let mesh = make_iso_circle_mesh(NPC_ATTACK_RANGE, 24);
        let mat = materials.add(ColorMaterial::from(Color::srgba(1.0, 0.25, 0.2, 0.30)));
        let s = world_to_screen(wp.0);
        commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(mat),
            Transform::from_xyz(s.x, s.y, depth_z(wp.0, MELEE_DECAL_LAYER)),
            MeleeArc {
                timer: Timer::from_seconds(MELEE_ARC_TTL, TimerMode::Once),
            },
        ));
    }
    // чистим память по исчезнувшим неписям, чтобы карта не росла
    prev.retain(|id, _| alive.contains(id));
}
