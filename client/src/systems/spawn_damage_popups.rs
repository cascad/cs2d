use crate::components::PlayerMarker;
use crate::events::PlayerDamagedEvent;
use crate::systems::iso::ISO_ACTOR_PX;
use crate::ui::components::DamagePopup;
use bevy::prelude::*;

/// Высота всплывающего урона над точкой ног — ВЫШЕ макушки и полоски HP
/// (та на ~0.82·ISO_ACTOR_PX), чтобы цифры не сливались с моделью.
const POPUP_Y_OFFSET: f32 = ISO_ACTOR_PX * 1.05;

pub fn spawn_damage_popups(
    mut commands: Commands,
    mut reader: MessageReader<PlayerDamagedEvent>,
    query: Query<(&PlayerMarker, &Transform)>,
    asset_server: Res<AssetServer>,
) {
    for ev in reader.read() {
        if let Some((_, tf)) = query.iter().find(|(m, _)| m.0 == ev.id) {
            let font = asset_server.load("fonts/FiraSans-Bold.ttf");

            commands.spawn((
                Text2d(format!("-{}", ev.damage)), // ← новая корректная инициализация
                TextFont {
                    font,
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.0, 0.0)),
                TextLayout::default(),
                Transform {
                    // над головой; z выше спрайтов и полоски HP, чтобы читалось
                    translation: Vec3::new(
                        tf.translation.x,
                        tf.translation.y + POPUP_Y_OFFSET,
                        950.0,
                    ),
                    ..default()
                },
                GlobalTransform::default(),
                DamagePopup {
                    timer: Timer::from_seconds(0.5, TimerMode::Once),
                },
            ));
        }
    }
}

pub fn update_damage_popups(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut DamagePopup, &mut Transform, &mut TextColor)>,
) {
    for (ent, mut popup, mut tf, mut color) in q.iter_mut() {
        popup.timer.tick(time.delta());

        tf.translation.y += 20.0 * time.delta_secs();

        let t = popup.timer.elapsed_secs() / popup.timer.duration().as_secs_f32();
        color.0.set_alpha(1.0 - t);

        if popup.timer.is_finished() {
            commands.entity(ent).despawn();
        }
    }
}
