use bevy::prelude::*;

use crate::components::Corpse;
use crate::systems::iso::KNIGHT_COLS;

/// Жизненный цикл трупа: проигрываем анимацию смерти рыцаря один раз (кадры
/// доходят до последнего и замирают). Труп НЕ исчезает по таймеру — лежит, пока
/// его не вытеснит лимит трупов (см. `Corpses`/`MAX_CORPSES`).
pub fn corpse_lifecycle(
    time: Res<Time>,
    mut q: Query<(&mut Corpse, &mut Sprite)>,
) {
    let dt = time.delta();
    for (mut corpse, mut spr) in q.iter_mut() {
        // покадровое проигрывание до последнего кадра, затем удержание
        if corpse.frame + 1 < KNIGHT_COLS && corpse.anim.tick(dt).just_finished() {
            corpse.frame += 1;
        }
        if let Some(atlas) = spr.texture_atlas.as_mut() {
            atlas.index = corpse.row * KNIGHT_COLS + corpse.frame;
        }
    }
}
