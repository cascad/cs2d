//! Клиентская часть НЕПИСЕЙ (скелеты и зомби): загрузка покадровых спрайтов
//! (8 направл. × 8 кадров отдельными PNG), спавн/обновление по снапшоту,
//! интерполяция позиции из буфера, покадровая анимация ходьбы, полоска HP и труп
//! (анимация смерти) по событию `NpcDied`. ИИ/логика общие; тип неписи (`NpcKind`)
//! задаёт лишь набор спрайтов и размер/якорь модели. Источник правды — сервер.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;

use crate::components::{Facing, NpcAnim, NpcHpFill, NpcKindC, NpcMarker};
use crate::render::{depth_z, layers, world_to_screen, world_to_translation, RenderLayer, WorldPos};
use crate::resources::{SnapshotBuffer, TimeSync};
use crate::systems::melee::{make_iso_circle_mesh, MeleeArc, MELEE_ARC_TTL, MELEE_DECAL_LAYER};
use crate::systems::utils::{hp_color, lerp_angle, time_in_seconds};
use protocol::constants::{NPC_ATTACK_RANGE, NPC_HP};
use protocol::messages::NpcKind;

/// Папки направлений в порядке экранных секторов: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
const DIR_NAMES: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
const NPC_FRAMES: usize = 8;
const NPC_DIRS: usize = 8;

/// Высота полоски HP над неписью (экранные px над точкой ног).
const NPC_HP_Y: f32 = 75.0;

/// Полоска HP неписи.
const NPC_HP_W: f32 = 40.0;
const NPC_HP_H: f32 = 5.0;

/// Время до деспавна неписи, пропавшей из снапшота: «потерял из виду — сразу
/// убрали». Единый порог с игроками (см. `fog::FADE_END`), чтобы непись и её
/// полоска HP не «зависали в воздухе». Крошечный — лишь сглаживает одиночный
/// потерянный пакет, на глаз это мгновенно.
use crate::systems::fog::FADE_END;

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

/// id неписи → сущность (для обновления/деспавна).
#[derive(Resource, Default)]
pub struct SpawnedNpcs(pub HashMap<u32, Entity>);

/// id неписи → (hp, aggro, attacking) из последнего снапшота — для полоски HP,
/// подсветки агра и проигрывания анимации атаки.
#[derive(Resource, Default)]
pub struct NpcInfo(pub HashMap<u32, (i32, bool, bool)>);

/// id неписи → время последнего появления в снапшоте (для угасания/деспавна).
#[derive(Resource, Default)]
pub struct NpcLastSeen(pub HashMap<u32, f64>);

/// Экранный 8-сектор из мирового угла (0=E..7=SE) — тем же способом, что и рыцарь.
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

/// Спавн сущности неписи (тело + полоска HP над головой).
fn spawn_npc_entity(
    commands: &mut Commands,
    anims: &NpcAnims,
    kind: NpcKind,
    id: u32,
    pos: Vec2,
    facing: f32,
) -> Entity {
    let vis = npc_visual(kind);
    let dir = npc_dir(facing);
    commands
        .spawn((
            Sprite {
                image: anims.get(kind).walk[dir][0].clone(),
                color: vis.base,
                custom_size: Some(Vec2::splat(vis.px)),
                ..default()
            },
            bevy::sprite::Anchor(Vec2::new(0.0, vis.anchor_y)),
            Transform::from_translation(world_to_translation(pos, layers::ACTOR)),
            GlobalTransform::default(),
            WorldPos(pos),
            RenderLayer(layers::ACTOR),
            NpcMarker(id),
            NpcKindC(kind),
            Facing(facing),
            NpcAnim::default(),
            Name::new(format!("Npc {id} ({:?})", kind)),
        ))
        .with_children(|p| {
            p.spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.65),
                    custom_size: Some(Vec2::new(NPC_HP_W + 2.0, NPC_HP_H + 2.0)),
                    ..default()
                },
                Transform::from_xyz(0.0, NPC_HP_Y, 0.05),
            ));
            p.spawn((
                Sprite {
                    color: hp_color(1.0),
                    custom_size: Some(Vec2::new(NPC_HP_W, NPC_HP_H)),
                    ..default()
                },
                bevy::sprite::Anchor(Vec2::new(-0.5, 0.0)),
                Transform::from_xyz(-NPC_HP_W * 0.5, NPC_HP_Y, 0.06),
                NpcHpFill { id, full_w: NPC_HP_W },
            ));
        })
        .id()
}

/// Применяет снапшот неписей: спавнит новых, помечает «видели», пишет hp/aggro.
pub fn apply_npc_snapshot(
    mut commands: Commands,
    buffer: Res<SnapshotBuffer>,
    anims: Option<Res<NpcAnims>>,
    mut spawned: ResMut<SpawnedNpcs>,
    mut info: ResMut<NpcInfo>,
    mut last_seen: ResMut<NpcLastSeen>,
) {
    let Some(anims) = anims else { return };
    let Some(snap) = buffer.snapshots.back() else { return };
    let now = time_in_seconds();
    for n in &snap.npcs {
        last_seen.0.insert(n.id, now);
        info.0.insert(n.id, (n.hp, n.aggro, n.attacking));
        if !spawned.0.contains_key(&n.id) {
            let e =
                spawn_npc_entity(&mut commands, &anims, n.kind, n.id, Vec2::new(n.x, n.y), n.facing);
            spawned.0.insert(n.id, e);
        }
    }
}

/// Интерполяция позиции/направления скелетов из буфера снапшотов (как у игроков).
pub fn interpolate_npcs(
    mut q: Query<(&mut WorldPos, &mut Facing, &NpcMarker)>,
    buffer: Res<SnapshotBuffer>,
    time_sync: Res<TimeSync>,
) {
    if buffer.snapshots.len() < 2 {
        return;
    }
    let now_s = time_in_seconds() - time_sync.offset;
    let rt = now_s - buffer.delay;
    let (mut prev, mut next) = (None, None);
    for snap in buffer.snapshots.iter() {
        if snap.server_time <= rt {
            prev = Some(snap);
        } else {
            next = Some(snap);
            break;
        }
    }
    let (prev, next) = match (prev, next) {
        (Some(p), Some(n)) => (p, n),
        (Some(p), None) => (p, p),
        _ => return,
    };
    let t0 = prev.server_time;
    let t1 = next.server_time.max(t0 + 1e-4);
    let alpha = ((rt - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;
    let mut pmap = HashMap::new();
    for n in &prev.npcs {
        pmap.insert(n.id, n);
    }
    let mut nmap = HashMap::new();
    for n in &next.npcs {
        nmap.insert(n.id, n);
    }
    for (mut wp, mut facing, marker) in q.iter_mut() {
        if let (Some(a), Some(b)) = (pmap.get(&marker.0), nmap.get(&marker.0)) {
            wp.0 = Vec2::new(a.x, a.y).lerp(Vec2::new(b.x, b.y), alpha);
            facing.0 = lerp_angle(a.facing, b.facing, alpha);
        }
    }
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
    let dt = time.delta();
    // порог «идёт» по перемещению за кадр (мир. ед.) — иначе считаем, что стоит
    const NPC_WALK_EPS: f32 = 0.15;
    for (wp, facing, kind, mut anim, mut sprite, marker) in q.iter_mut() {
        let moving = (wp.0 - anim.prev).length() > NPC_WALK_EPS;
        anim.prev = wp.0;
        let (aggro, attacking) = info
            .0
            .get(&marker.0)
            .map(|(_, a, atk)| (*a, *atk))
            .unwrap_or((false, false));

        let vis = npc_visual(kind.0);
        let dir = npc_dir(facing.0);
        let set = anims.get(kind.0);

        // выбор клипа по приоритету: атака → ходьба → покой. Атака и покой всегда
        // проигрываются (зацикленно), ходьба — только когда непись реально идёт.
        let (frames, advancing) = if attacking && set.attack.is_some() {
            (set.attack.as_ref().unwrap(), true)
        } else if moving {
            (&set.walk, true)
        } else if let Some(idle) = set.idle.as_ref() {
            (idle, true)
        } else {
            // у скелета нет покоя — стоим на кадре 0 ходьбы (без «марша на месте»)
            (&set.walk, false)
        };

        if advancing {
            if anim.timer.tick(dt).just_finished() {
                anim.frame = (anim.frame + 1) % NPC_FRAMES;
            }
        } else {
            anim.frame = 0;
        }
        sprite.image = frames[dir][anim.frame.min(NPC_FRAMES - 1)].clone();
        sprite.color = if aggro { vis.aggro } else { vis.base };
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

/// Угасание и деспавн скелетов, выпавших из снапшота (туман войны/смерть).
pub fn fade_unseen_npcs(
    mut commands: Commands,
    mut last_seen: ResMut<NpcLastSeen>,
    mut spawned: ResMut<SpawnedNpcs>,
    mut info: ResMut<NpcInfo>,
    q: Query<(Entity, &NpcMarker)>,
) {
    let now = time_in_seconds();
    let mut gone: Vec<u32> = Vec::new();
    for (e, marker) in q.iter() {
        let age = last_seen.0.get(&marker.0).map(|&t| now - t).unwrap_or(0.0);
        if age >= FADE_END {
            commands.entity(e).despawn();
            gone.push(marker.0);
        }
    }
    for id in gone {
        spawned.0.remove(&id);
        last_seen.0.remove(&id);
        info.0.remove(&id);
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
