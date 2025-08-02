use crate::components::{Explosion, Grenade};
use bevy::prelude::*;

/// Движение гранаты и её взрыв
pub fn grenade_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Grenade)>,
) {
    let dt = time.delta_secs();
    for (ent, mut tf, mut gr) in q.iter_mut() {
        // движение
        tf.translation += (gr.dir * gr.speed * dt).extend(0.0);

        // таймер
        gr.timer.tick(time.delta());
        if gr.timer.just_finished() {
            // удаляем гранату
            commands.entity(ent).despawn();

            // todo
            // Если позже почините фичи и захотите красивую текстуру/анимацию — просто добавьте bevy_asset, подмените пункт 3 (спрайт) на SpriteBundle + texture: my_handle.clone().

            // создаём взрыв
            commands.spawn((
                // спрайт‑круг (цвет + радиус)
                Sprite {
                    color: Color::linear_rgba(1.0, 0.6, 0.2, 0.8),
                    custom_size: Some(Vec2::splat(gr.blast_radius * 2.0)),
                    ..default()
                },
                Transform::from_translation(tf.translation),
                GlobalTransform::default(),
                Explosion {
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                },
            ));
        }
    }
}

pub fn explosion_lifecycle(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Explosion, &mut Sprite)>,
) {
    for (ent, mut exp, mut sprite) in q.iter_mut() {
        exp.timer.tick(time.delta());
        let t = exp.timer.elapsed_secs() / exp.timer.duration().as_secs_f32();
        // плавно прозрачно исчезает
        sprite.color.set_alpha(1.0 - t);
        if exp.timer.finished() {
            commands.entity(ent).despawn();
        }
    }
}
