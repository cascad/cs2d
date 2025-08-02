use bevy::prelude::*;

#[derive(Component)]
pub struct ExplosionMaterial(pub Handle<ColorMaterial>);

#[derive(Component)]
pub struct DamagePopup {
    pub timer: Timer,
}

#[derive(Component)]
pub struct PlayerHpUi {
    pub player_id: u64,
}