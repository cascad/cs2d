use crate::{components::LocalPlayer, resources::MyPlayer, systems::utils::time_in_seconds};
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::{
    constants::CH_C2S,
    messages::{C2S, GrenadeEvent},
};

pub fn grenade_throw(
    keys: Res<ButtonInput<KeyCode>>,
    my: Res<MyPlayer>,
    mut client: ResMut<QuinnetClient>,
    query: Query<&Transform, With<LocalPlayer>>,
) {
    if keys.just_pressed(KeyCode::KeyG) {
        // Пытаемся достать трансформ локального игрока
        match query.single() {
            Ok(transform) => {
                let pos = transform.translation.truncate();
                // Направление—просто вправо, или по вашему выбору
                let dir = Vec2::new(1.0, 0.0);
                let ts = time_in_seconds();
                let ev = GrenadeEvent {
                    id: my.id ^ (ts as u64),
                    from: pos,
                    dir,
                    speed: 500.0,
                    timer: 3.0,
                    timestamp: ts,
                };
                if client
                    .connection_mut()
                    .send_message_on(CH_C2S, C2S::ThrowGrenade(ev.clone()))
                    .is_ok()
                {
                    info!("💣 Sent ThrowGrenade {}", ev.id);
                }
            }
            Err(_) => {
                // Локальный игрок ещё не заспавнен
                warn!("🔸 grenade_throw: no LocalPlayer entity found");
            }
        }
    }
}
