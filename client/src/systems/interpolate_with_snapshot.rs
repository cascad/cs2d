use crate::components::PlayerMarker;
use crate::resources::{MyPlayer, SnapshotBuffer, TimeSync};
use crate::systems::utils::{lerp_angle, stance_color, time_in_seconds};
use bevy::prelude::*;
use protocol::messages::Stance;
use std::collections::HashMap;

pub fn interpolate_with_snapshot(
    mut q: Query<(&mut Transform, &mut Sprite, &PlayerMarker)>,
    buffer: Res<SnapshotBuffer>,
    my: Res<MyPlayer>,
    time_sync: Res<TimeSync>,
) {
    if buffer.snapshots.len() < 2 {
        return;
    }
    let now_s = time_in_seconds() - time_sync.offset;
    let rt = now_s - buffer.delay;
    let (mut prev, mut next) = (None, None);
    for snap in buffer.snapshots.iter() {
        if snap.server_time <= rt {
            prev = Some(snap);
        } else {
            next = Some(snap);
            break;
        }
    }
    let (prev, next) = match (prev, next) {
        (Some(p), Some(n)) => (p, n),
        (Some(p), None) => (p, p),
        _ => return,
    };
    let t0 = prev.server_time;
    let t1 = next.server_time.max(t0 + 1e-4);
    let alpha = ((rt - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;
    let mut pmap = HashMap::new();
    for p in &prev.players {
        pmap.insert(p.id, p);
    }
    let mut nmap = HashMap::new();
    for p in &next.players {
        nmap.insert(p.id, p);
    }
    for (mut t, mut s, marker) in q.iter_mut() {
        if marker.0 == my.id {
            continue;
        }
        if let (Some(p0), Some(p1)) = (pmap.get(&marker.0), nmap.get(&marker.0)) {
            let from = Vec2::new(p0.x, p0.y);
            let to = Vec2::new(p1.x, p1.y);
            t.translation = from.lerp(to, alpha).extend(0.0);
            t.rotation = Quat::from_rotation_z(lerp_angle(p0.rotation, p1.rotation, alpha));
            s.color = stance_color(&p1.stance);
        }
    }
}

pub fn interpolate_with_snapshot_old(
    mut q: Query<(&mut Transform, &mut Sprite, &PlayerMarker)>,
    buffer: Res<SnapshotBuffer>,
    my: Res<MyPlayer>,
    time_sync: Res<TimeSync>,
    time: Res<Time>, // для fallback времени, если нужно
) {
    // Нужны как минимум два снапшота
    if buffer.snapshots.len() < 2 {
        return;
    }

    // Текущее "серверное" время, скорректированное на client_time + offset
    let now_client = time.elapsed_secs_f64();
    let target_time = now_client - time_sync.offset - buffer.delay;

    // Найдём два снапшота: prev.server_time <= target_time < next.server_time
    let mut prev_snap = &buffer.snapshots[0];
    let mut next_snap = &buffer.snapshots[1];
    for window in buffer.snapshots.as_slices().0.windows(2) {
        let a = &window[0];
        let b = &window[1];
        if a.server_time <= target_time && target_time < b.server_time {
            prev_snap = a;
            next_snap = b;
            break;
        }
    }

    let t0 = prev_snap.server_time;
    let t1 = next_snap.server_time.max(t0 + 1e-6);
    let alpha = ((target_time - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;

    // Построим карты id → PlayerSnapshot для prev и next
    let mut prev_map = std::collections::HashMap::new();
    for p in &prev_snap.players {
        prev_map.insert(p.id, p);
    }
    let mut next_map = std::collections::HashMap::new();
    for p in &next_snap.players {
        next_map.insert(p.id, p);
    }

    // Интерполируем всех "чужих" игроков
    for (mut transform, mut sprite, marker) in q.iter_mut() {
        // Пропускаем локального
        if marker.0 == my.id {
            continue;
        }
        // Берём старую и новую позицию
        if let (Some(p0), Some(p1)) = (prev_map.get(&marker.0), next_map.get(&marker.0)) {
            let from = Vec2::new(p0.x, p0.y);
            let to = Vec2::new(p1.x, p1.y);
            // Линейно интерполируем позицию
            let pos = from.lerp(to, alpha);
            transform.translation = pos.extend(transform.translation.z);

            // Интерполируем поворот (угол)
            let rot = lerp_angle(p0.rotation, p1.rotation, alpha);
            transform.rotation = Quat::from_rotation_z(rot);

            // (Опционально) интерполировать цвет по смене стойки
            sprite.color = match &p1.stance {
                Stance::Standing => Color::srgb(0.20, 1.00, 0.20),
                Stance::Crouching => Color::srgb(0.15, 0.85, 1.00),
                Stance::Prone => Color::srgb(0.00, 0.60, 0.60),
            };
        }
    }
}
