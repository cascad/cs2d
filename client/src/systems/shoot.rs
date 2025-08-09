use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_quinnet::client::QuinnetClient;
use crate::components::{LocalPlayer, Bullet};
use crate::constants::{BULLET_SPEED, BULLET_TTL};
use crate::resources::{MyPlayer};
use crate::systems::utils::time_in_seconds;
use protocol::messages::{ShootEvent, C2S};
use protocol::constants::{CH_C2S};

pub fn shoot_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    player_q: Query<&Transform, With<LocalPlayer>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    println!("🖱 [Client] Mouse Left pressed");

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => {
            println!("⚠️ [Client] No window");
            return;
        }
    };
    let cursor = match window.cursor_position() {
        Some(c) => c,
        None => {
            println!("⚠️ [Client] No cursor pos");
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
    let world = match camera.viewport_to_world_2d(cam_tf, cursor) {
        Ok(p) => p,
        Err(_) => {
            println!("⚠️ [Client] Failed world transform");
            return;
        }
    };
    let player_pos = match player_q.single() {
        Ok(t) => t.translation.truncate(),
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

fn spawn_tracer(commands: &mut Commands, from: Vec2, dir: Vec2) {
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(12.0, 2.0)),
            ..default()
        },
        Transform::from_translation(from.extend(10.0))
            .with_rotation(Quat::from_rotation_z(dir.y.atan2(dir.x))),
        GlobalTransform::default(),
        Bullet {
            ttl: BULLET_TTL,
            vel: dir * BULLET_SPEED,
        },
    ));
}