use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::QuinnetClient;
use protocol::abilities::AbilityConfig;
use protocol::constants::{CH_C2S, MELEE_HALF_ANGLE};
use protocol::messages::{C2S, MeleeEvent};

use crate::components::{LocalPlayer, MeleeSwing};
use crate::render::{layers, pointer_world, world_to_translation, RenderLayer, WorldPos};
use crate::resources::{LocalAbilities, MeleeMesh, MyPlayer};
use crate::systems::utils::time_in_seconds;

/// Базовый цвет взмаха (тёплый «огненный»).
fn swing_color(alpha: f32) -> Color {
    Color::srgba(1.0, 0.65, 0.18, alpha)
}
const SWING_ALPHA: f32 = 0.9;
const SWING_TTL: f32 = 0.12; // мгновенно появился, быстро прочертил и исчез

/// Спавнит «прочерк» в позиции `pos` по направлению `dir`: узкое лезвие,
/// которое за `SWING_TTL` пробегает по конусу ±`MELEE_HALF_ANGLE`.
/// Каждому взмаху — свой материал, чтобы затухание не влияло на остальные.
pub fn spawn_melee_swing(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    pos: Vec2,
    dir: Vec2,
) {
    let d = dir.normalize_or_zero();
    if d == Vec2::ZERO {
        return;
    }
    let facing = d.y.atan2(d.x);
    let half = MELEE_HALF_ANGLE;
    let dir_sign = 1.0; // прочерк от одного края конуса к другому
    let material = materials.add(ColorMaterial::from_color(swing_color(SWING_ALPHA)));
    // стартуем с края конуса
    let start = facing - dir_sign * half;
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_translation(world_to_translation(pos, layers::EFFECT))
            .with_rotation(Quat::from_rotation_z(start)),
        WorldPos(pos),
        RenderLayer(layers::EFFECT),
        MeleeSwing {
            timer: Timer::from_seconds(SWING_TTL, TimerMode::Once),
            facing,
            half,
            dir_sign,
        },
    ));
}

/// Ближний удар по ПКМ (Mouse2): шлём запрос серверу и СРАЗУ рисуем взмах
/// локально (не дожидаясь ответа сервера — иначе визуал «запаздывает» на RTT).
pub fn melee_attack(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
    my: Res<MyPlayer>,
    mut abilities: ResMut<LocalAbilities>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
    melee_mesh: Res<MeleeMesh>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let cfg = AbilityConfig::default();
    if !abilities.0.melee_ready(&cfg) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cam_q.single() else { return };
    let Some(world) = pointer_world(window, camera, cam_tf) else { return };
    let Ok(wp) = player_q.single() else { return };

    let player_pos = wp.0;
    let dir = (world - player_pos).normalize_or_zero();
    if dir == Vec2::ZERO {
        return;
    }

    let ev = MeleeEvent {
        attacker_id: my.id,
        dir,
        timestamp: time_in_seconds(),
    };
    if client
        .connection_mut()
        .send_message_on(CH_C2S, C2S::Melee(ev))
        .is_ok()
    {
        abilities.0.consume_melee(&cfg);
        // мгновенный локальный визуал
        spawn_melee_swing(&mut commands, melee_mesh.0.clone(), &mut materials, player_pos, dir);
    }
}

/// Анимация прочерка: лезвие вращается по конусу от края к краю и гаснет к концу.
pub fn melee_swing_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(
        Entity,
        &mut MeleeSwing,
        &mut Transform,
        &MeshMaterial2d<ColorMaterial>,
    )>,
) {
    for (e, mut sw, mut tf, mat) in q.iter_mut() {
        sw.timer.tick(time.delta());
        let left = sw.timer.fraction_remaining();
        let progress = 1.0 - left; // 0 → 1 по ходу взмаха

        // лезвие бежит по конусу: от (facing - half) к (facing + half)
        let angle = sw.facing - sw.dir_sign * sw.half + sw.dir_sign * (2.0 * sw.half) * progress;
        tf.rotation = Quat::from_rotation_z(angle);

        // яркое во время прочерка, быстрый спад в конце
        let alpha = if left > 0.4 {
            SWING_ALPHA
        } else {
            SWING_ALPHA * (left / 0.4)
        };
        if let Some(m) = materials.get_mut(&mat.0) {
            m.color = swing_color(alpha);
        }

        if sw.timer.finished() {
            commands.entity(e).despawn();
        }
    }
}
