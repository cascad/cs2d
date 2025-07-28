use bevy::prelude::*;

#[derive(Component)]
pub struct LocalPlayer;

#[derive(Component)]
pub struct PlayerMarker(pub u64);

#[derive(Component)]
pub struct Bullet {
    pub ttl: f32,
    pub vel: Vec2,
}

#[derive(Component)]
pub struct GrenadeMarker(pub u64);

#[derive(Component)]
pub struct GrenadeTimer(pub Timer);

#[derive(Component)]
pub struct Health(pub i32);