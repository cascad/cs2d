use bevy::prelude::*;
use protocol::constants::TILE_SIZE;
use protocol::geom::WallGrid;

use crate::{
    resources::{WallAabbCache, WallGridRes},
    systems::level::Wall,
};

pub fn build_wall_aabb_cache(
    mut cache: ResMut<WallAabbCache>,
    mut grid: ResMut<WallGridRes>,
    q: Query<(&Transform, &Sprite), With<Wall>>,
) {
    // Стены статичны: как только кэш собран — больше не пересобираем.
    // Пока стены ещё не заспавнены (кэш пуст и запрос пуст) — просто ждём следующего кадра.
    if !cache.0.is_empty() {
        return;
    }

    let mut out = Vec::new();
    for (t, s) in q.iter() {
        if let Some(size) = s.custom_size {
            let half = size / 2.0 - Vec2::splat(0.001);
            let c = t.translation.truncate();
            out.push((c - half, c + half));
        }
    }
    // Спатиал-сетка для рейкастов (ячейка = 2 тайла), строится один раз вместе с кэшем.
    grid.0 = WallGrid::build(&out, TILE_SIZE * 2.0);
    cache.0 = out;
}
