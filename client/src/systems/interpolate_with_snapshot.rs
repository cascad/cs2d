use crate::components::PlayerMarker;
use crate::resources::{MyPlayer, SnapshotBuffer, TimeSync};
use crate::systems::utils::{lerp_angle, stance_color, time_in_seconds};
use bevy::prelude::*;
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
            // todo need for stance texture, change here
            // s.color = stance_color(&p1.stance);
        }
    }
}
