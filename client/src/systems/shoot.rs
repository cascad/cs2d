use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::QuinnetClient;
use crate::components::{LocalPlayer, Bullet};
use crate::constants::BULLET_SPEED;
use crate::render::{layers, pointer_world, world_to_translation, RenderLayer, WorldPos};
use crate::resources::{MyPlayer};
use crate::systems::utils::time_in_seconds;
use protocol::messages::{ShootEvent, C2S};
use protocol::constants::{CH_C2S};

pub fn shoot_mouse(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&WorldPos, With<LocalPlayer>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
) {
    // стрельба перенесена с mouse1 на Left Shift (mouse1 теперь melee)
    if !keys.just_pressed(KeyCode::ShiftLeft) {
        return;
    }
    println!("🔫 [Client] Shoot key (LShift) pressed");

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => {
            println!("⚠️ [Client] No window");
            return;
        }
    };
    let (camera, cam_tf) = match cam_q.single() {
        Ok(c) => c,
        Err(_) => {
            println!("⚠️ [Client] No camera");
            return;
        }
    };
    let world = match pointer_world(window, camera, cam_tf) {
        Some(p) => p,
        None => {
            println!("⚠️ [Client] No cursor/world");
            return;
        }
    };
    let player_pos = match player_q.single() {
        Ok(wp) => wp.0,
        Err(err) => {
            println!("⚠️ [Client] No LocalPlayer: {:?}", err);
            return;
        }
    };
    let dir = (world - player_pos).normalize_or_zero();

    let shoot = ShootEvent {
        shooter_id: my.id,
        dir,
        timestamp: time_in_seconds(),
    };
    match client
        .connection_mut()
        .send_message_on(CH_C2S, C2S::Shoot(shoot.clone()))
    {
        Ok(_) => println!("📤 [Client] Sent ShootEvent: {:?}", shoot),
        Err(e) => println!("❌ [Client] Shoot send error: {:?}", e),
    };
    println!("🎨 [Client] Local spawn_tracer");
    // трассер рисуется по ивенту, тут не нужен
    // spawn_tracer(&mut commands, player_pos, dir);
}

pub fn spawn_tracer(commands: &mut Commands, from: Vec2, dir: Vec2, ttl:f32) {
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(12.0, 2.0)),
            ..default()
        },
        Transform::from_translation(world_to_translation(from, layers::PROJECTILE))
            .with_rotation(Quat::from_rotation_z(dir.y.atan2(dir.x))),
        GlobalTransform::default(),
        WorldPos(from),
        RenderLayer(layers::PROJECTILE),
        Bullet {
            ttl: ttl,
            vel: dir * BULLET_SPEED,
        },
    ));
}