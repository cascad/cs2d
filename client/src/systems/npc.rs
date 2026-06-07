//! Клиентская часть НЕПИСЕЙ (скелетов): загрузка покадровых спрайтов (8 направл.
//! × 8 кадров отдельными PNG, ассеты Lords of Pain), спавн/обновление по снапшоту,
//! интерполяция позиции из буфера, покадровая анимация ходьбы, полоска HP и труп
//! (анимация смерти) по событию `NpcDied`. Источник правды — сервер.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::components::{Facing, NpcAnim, NpcHpFill, NpcMarker};
use crate::render::{layers, world_to_screen, world_to_translation, RenderLayer, WorldPos};
use crate::resources::{SnapshotBuffer, TimeSync};
use crate::systems::utils::{hp_color, lerp_angle, time_in_seconds};
use protocol::constants::NPC_HP;

/// Папки направлений в порядке экранных секторов: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
const DIR_NAMES: [&str; 8] = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"];
const NPC_FRAMES: usize = 8;
const NPC_DIRS: usize = 8;

/// Экранный размер спрайта скелета. Кадр 256×256, но сам скелет занимает лишь
/// ~37px по высоте (центр кадра), поэтому квадрат большой, чтобы скелет вышел
/// размером с игрока. Якорь — почти по центру (ноги чуть ниже центра).
/// +25% к прежним 350 → ~440 (скелет чуть крупнее, вровень/чуть выше игрока).
const NPC_PX: f32 = 440.0;
const NPC_ANCHOR_Y: f32 = -0.04;

/// Высота полоски HP над скелетом (экранные px над точкой ног).
const NPC_HP_Y: f32 = 75.0;

/// Полоска HP скелета.
const NPC_HP_W: f32 = 40.0;
const NPC_HP_H: f32 = 5.0;

/// Время до деспавна скелета, пропавшего из снапшота. Маленькое: «потерял из
/// виду — сразу убрали», без «марша на месте». Чуть больше нуля, чтобы не мигал
/// на одиночных пропусках снапшота.
const FADE_END: f64 = 0.18;

/// Хэндлы кадров скелета: walk/death, индексация [направление][кадр].
#[derive(Resource, Clone)]
pub struct SkeletonAnims {
    pub walk: Vec<[Handle<Image>; NPC_FRAMES]>,
    pub death: Vec<[Handle<Image>; NPC_FRAMES]>,
}

/// id неписи → сущность (для обновления/деспавна).
#[derive(Resource, Default)]
pub struct SpawnedNpcs(pub HashMap<u32, Entity>);

/// id неписи → (hp, aggro) из последнего снапшота — для полоски HP и подсветки.
#[derive(Resource, Default)]
pub struct NpcInfo(pub HashMap<u32, (i32, bool)>);

/// id неписи → время последнего появления в снапшоте (для угасания/деспавна).
#[derive(Resource, Default)]
pub struct NpcLastSeen(pub HashMap<u32, f64>);

/// Экранный 8-сектор из мирового угла (0=E..7=SE) — тем же способом, что и рыцарь.
#[inline]
pub fn skeleton_dir(facing_world: f32) -> usize {
    let s = world_to_screen(Vec2::new(facing_world.cos(), facing_world.sin()));
    let deg = s.y.atan2(s.x).to_degrees().rem_euclid(360.0);
    ((deg / 45.0).round() as i32).rem_euclid(8) as usize
}

/// Загрузка всех кадров скелета один раз на старте.
pub fn setup_skeletons(mut commands: Commands, asset_server: Res<AssetServer>) {
    let load_anim = |name: &str| -> Vec<[Handle<Image>; NPC_FRAMES]> {
        (0..NPC_DIRS)
            .map(|d| {
                std::array::from_fn(|f| {
                    asset_server.load(format!("skeleton/{}/{}/{}.png", name, DIR_NAMES[d], f))
                })
            })
            .collect()
    };
    commands.insert_resource(SkeletonAnims {
        walk: load_anim("walk"),
        death: load_anim("death"),
    });
}

/// Спавн сущности скелета (тело + полоска HP над головой).
fn spawn_skeleton(
    commands: &mut Commands,
    anims: &SkeletonAnims,
    id: u32,
    pos: Vec2,
    facing: f32,
) -> Entity {
    let dir = skeleton_dir(facing);
    commands
        .spawn((
            Sprite {
                image: anims.walk[dir][0].clone(),
                color: Color::srgb(1.6, 1.6, 1.65), // посветлее
                custom_size: Some(Vec2::splat(NPC_PX)),
                ..default()
            },
            bevy::sprite::Anchor(Vec2::new(0.0, NPC_ANCHOR_Y)),
            Transform::from_translation(world_to_translation(pos, layers::ACTOR)),
            GlobalTransform::default(),
            WorldPos(pos),
            RenderLayer(layers::ACTOR),
            NpcMarker(id),
            Facing(facing),
            NpcAnim::default(),
            Name::new(format!("Skeleton {id}")),
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
    anims: Option<Res<SkeletonAnims>>,
    mut spawned: ResMut<SpawnedNpcs>,
    mut info: ResMut<NpcInfo>,
    mut last_seen: ResMut<NpcLastSeen>,
) {
    let Some(anims) = anims else { return };
    let Some(snap) = buffer.snapshots.back() else { return };
    let now = time_in_seconds();
    for n in &snap.npcs {
        last_seen.0.insert(n.id, now);
        info.0.insert(n.id, (n.hp, n.aggro));
        if !spawned.0.contains_key(&n.id) {
            let e = spawn_skeleton(&mut commands, &anims, n.id, Vec2::new(n.x, n.y), n.facing);
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

/// Покадровая анимация ходьбы скелета + подсветка агра.
pub fn animate_skeletons(
    time: Res<Time>,
    anims: Option<Res<SkeletonAnims>>,
    info: Res<NpcInfo>,
    mut q: Query<(&WorldPos, &Facing, &mut NpcAnim, &mut Sprite, &NpcMarker)>,
) {
    let Some(anims) = anims else { return };
    let dt = time.delta();
    // порог «идёт» по перемещению за кадр (мир. ед.) — иначе считаем, что стоит
    const NPC_WALK_EPS: f32 = 0.15;
    for (wp, facing, mut anim, mut sprite, marker) in q.iter_mut() {
        let moving = (wp.0 - anim.prev).length() > NPC_WALK_EPS;
        anim.prev = wp.0;
        if moving {
            if anim.timer.tick(dt).just_finished() {
                anim.frame = (anim.frame + 1) % NPC_FRAMES;
            }
        } else {
            // стоит (в т.ч. когда снапшоты замерли вне обзора) — кадр покоя, без марша
            anim.frame = 0;
        }
        let dir = skeleton_dir(facing.0);
        sprite.image = anims.walk[dir][anim.frame].clone();
        // агр — лёгкий красноватый оттенок (всё ещё светлый)
        let aggro = info.0.get(&marker.0).map(|(_, a)| *a).unwrap_or(false);
        sprite.color = if aggro {
            Color::srgb(1.7, 1.35, 1.3)
        } else {
            Color::srgb(1.6, 1.6, 1.65)
        };
    }
}

/// Обновляет ширину полоски HP скелета из `NpcInfo`.
pub fn update_npc_hp_bars(
    info: Res<NpcInfo>,
    mut q: Query<(&NpcHpFill, &mut Sprite, &mut Transform)>,
) {
    for (fill, mut sprite, mut tf) in q.iter_mut() {
        let hp = info.0.get(&fill.id).map(|(h, _)| *h).unwrap_or(0);
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

/// Спавн трупа скелета (анимация смерти, замирает на последнем кадре). Возвращает
/// сущность, чтобы зарегистрировать её в общем лимите трупов (`Corpses`).
pub fn spawn_skeleton_corpse(
    commands: &mut Commands,
    anims: &SkeletonAnims,
    pos: Vec2,
    facing: f32,
) -> Entity {
    let dir = skeleton_dir(facing);
    commands
        .spawn((
            Sprite {
                image: anims.death[dir][0].clone(),
                color: Color::srgb(1.5, 1.5, 1.55),
                custom_size: Some(Vec2::splat(NPC_PX)),
                ..default()
            },
            bevy::sprite::Anchor(Vec2::new(0.0, NPC_ANCHOR_Y)),
            Transform::from_translation(world_to_translation(pos, layers::CORPSE)),
            GlobalTransform::default(),
            WorldPos(pos),
            RenderLayer(layers::CORPSE),
            SkeletonCorpse {
                dir,
                frame: 0,
                anim: Timer::from_seconds(1.0 / 12.0, TimerMode::Repeating),
            },
        ))
        .id()
}

/// Труп скелета: проигрывает анимацию смерти один раз и держит последний кадр.
/// НЕ исчезает по таймеру — лежит, пока не вытеснен лимитом трупов.
#[derive(Component)]
pub struct SkeletonCorpse {
    pub dir: usize,
    pub frame: usize,
    pub anim: Timer,
}

/// Прогон анимации трупа скелета (до последнего кадра, затем удержание).
pub fn animate_skeleton_corpses(
    time: Res<Time>,
    anims: Option<Res<SkeletonAnims>>,
    mut q: Query<(&mut SkeletonCorpse, &mut Sprite)>,
) {
    let Some(anims) = anims else { return };
    let dt = time.delta();
    for (mut corpse, mut sprite) in q.iter_mut() {
        if corpse.frame < NPC_FRAMES - 1 && corpse.anim.tick(dt).just_finished() {
            corpse.frame += 1;
        }
        sprite.image = anims.death[corpse.dir][corpse.frame].clone();
    }
}
