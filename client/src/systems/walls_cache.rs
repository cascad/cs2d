use bevy::prelude::*;

use crate::{resources::WallAabbCache, systems::level::Wall};

pub fn build_wall_aabb_cache(
    mut cache: ResMut<WallAabbCache>,
    q: Query<(&Transform, &Sprite), With<Wall>>,
) {
    let mut out = Vec::new();
    for (t, s) in q.iter() {
        if let Some(size) = s.custom_size {
            let half = size / 2.0 - Vec2::splat(0.001);
            let c = t.translation.truncate();
            out.push((c - half, c + half));
        }
    }
    cache.0 = out;
    // todo need to cache only once
    // info!("🧱 cached {} wall AABBs", cache.0.len());
}
