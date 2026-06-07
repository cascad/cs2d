use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::QuinnetClient;
use protocol::abilities::AbilityConfig;
use protocol::constants::{CH_C2S, MELEE_HALF_WIDTH, MELEE_RANGE};
use protocol::messages::{C2S, MeleeEvent};

use crate::components::{ActorAnim, AnimState, LocalPlayer};
use crate::render::{depth_z, world_to_screen, pointer_world, WorldPos};
use crate::resources::{LocalAbilities, MyPlayer};
use crate::systems::utils::time_in_seconds;

/// Слой декали удара: над полом/стенами (видно на земле), но ПОД актёром (рыцарь
/// стоит поверх неё). Между CORPSE(150) и ACTOR(200).
const MELEE_DECAL_LAYER: f32 = 170.0;
/// Время жизни/затухания полуокружности удара.
const MELEE_ARC_TTL: f32 = 0.22;

/// Декаль сектора урона ближнего боя на земле — гаснет за [`MELEE_ARC_TTL`].
#[derive(Component)]
pub struct MeleeArc {
    pub timer: Timer,
}

/// Ближний удар по ЛКМ (Mouse1): шлём запрос серверу, СРАЗУ запускаем анимацию
/// удара рыцаря и рисуем под ним полуокружность реальной зоны урона.
pub fn melee_attack(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut player_q: Query<(&WorldPos, &mut ActorAnim), With<LocalPlayer>>,
    my: Res<MyPlayer>,
    mut abilities: ResMut<LocalAbilities>,
    mut client: ResMut<QuinnetClient>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
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
    let Ok((wp, mut anim)) = player_q.single_mut() else { return };

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
        anim.start_action(AnimState::Attack);

        // Реальная зона урона — ПОЛОСА (капсула) ПОСТОЯННОЙ ширины 2·MELEE_HALF_WIDTH
        // вдоль взгляда, от ЦЕНТРА игрока; изо-спроецирована и лежит на земле.
        let dir_angle = dir.y.atan2(dir.x);
        let mesh = make_iso_capsule_mesh(dir_angle, MELEE_RANGE, MELEE_HALF_WIDTH, 10);
        let mat = materials.add(ColorMaterial::from(Color::srgba(1.0, 0.82, 0.25, 0.32)));
        let s = world_to_screen(player_pos);
        commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(mat),
            Transform::from_xyz(s.x, s.y, depth_z(player_pos, MELEE_DECAL_LAYER)),
            MeleeArc {
                timer: Timer::from_seconds(MELEE_ARC_TTL, TimerMode::Once),
            },
        ));
    }
}

/// Затухание и удаление декали удара.
pub fn melee_arc_lifecycle(
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(Entity, &mut MeleeArc, &MeshMaterial2d<ColorMaterial>)>,
    mut commands: Commands,
) {
    for (e, mut arc, mat) in q.iter_mut() {
        arc.timer.tick(time.delta());
        let frac = 1.0 - arc.timer.fraction();
        if let Some(m) = materials.get_mut(&mat.0) {
            m.color.set_alpha(0.32 * frac);
        }
        if arc.timer.is_finished() {
            commands.entity(e).despawn();
        }
    }
}

/// Заливочная «капсула» (stadium) ПОСТОЯННОЙ ширины вдоль направления удара,
/// спроецированная в изометрию: контур считаем в мире, проецируем `world_to_screen`
/// (сжатие 2:1 «запекается» в геометрию), треугольники — веером из центра игрока
/// (он внутри капсулы, фигура выпуклая). Совпадает с серверной зоной `in_melee_swath`.
fn make_iso_capsule_mesh(dir_angle: f32, length: f32, half_width: f32, segments: usize) -> Mesh {
    use std::f32::consts::{FRAC_PI_2, PI};
    let f = Vec2::new(dir_angle.cos(), dir_angle.sin());
    let far = f * length;

    // контур: дальняя полуокружность (da-90°..da+90°) + ближняя (da+90°..da+270°)
    let mut hull: Vec<Vec2> = Vec::with_capacity(2 * (segments + 1));
    for i in 0..=segments {
        let a = dir_angle - FRAC_PI_2 + PI * (i as f32 / segments as f32);
        hull.push(far + Vec2::new(a.cos(), a.sin()) * half_width);
    }
    for i in 0..=segments {
        let a = dir_angle + FRAC_PI_2 + PI * (i as f32 / segments as f32);
        hull.push(Vec2::new(a.cos(), a.sin()) * half_width);
    }

    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]]; // центр (точка игрока)
    for w in &hull {
        let s = world_to_screen(*w);
        positions.push([s.x, s.y, 0.0]);
    }
    let n = hull.len() as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(n as usize * 3);
    for i in 0..n {
        let a = 1 + i;
        let b = 1 + (i + 1) % n; // замыкаем контур
        indices.extend([0u32, a, b]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
