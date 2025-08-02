use bevy::prelude::*;

#[derive(Resource)]
/// Хэндлы на текстуры гранаты и анимацию взрыва
pub struct ExplosionTextures {
    /// Спрайт самой гранаты
    pub grenade: Handle<Image>,
    /// Текстура-круг для эффекта взрыва
    pub explosion_circle: Handle<Image>,
}