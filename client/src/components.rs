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
pub struct Health(pub i32);

/// Компонент «летящая граната»
#[derive(Component)]
pub struct Grenade {
    pub id: u64,
    pub from: Vec2,
    /// Нормализованное направление полёта
    pub dir: Vec2,
    /// Скорость (пикселей в секунду)
    pub speed: f32,
    /// Таймер до взрыва
    pub timer: Timer,
    /// Радиус взрыва (в тех же единицах, что и мир)
    pub blast_radius: f32,
}

#[derive(Component)]
/// Эффект взрыва
pub struct Explosion {
    /// Таймер длительности эффекта
    pub timer: Timer,
}

/// Компонент на визуальной гранате: связывает сущность с id гранаты на сервере
#[derive(Component)]
pub struct GrenadeNet {
    pub id: u64,
}

#[derive(Component)]
pub struct Corpse {
    pub timer: Timer, // сколько лежит труп
}

/// Кратковременный визуал ближнего удара — «прочерк»: узкое лезвие пробегает
/// по конусу от одного края к другому за время `timer` и гаснет.
#[derive(Component)]
pub struct MeleeSwing {
    pub timer: Timer,
    pub facing: f32,   // направление взгляда (центр конуса), рад
    pub half: f32,     // полу-угол конуса = амплитуда прочерка, рад
    pub dir_sign: f32, // направление прочерка: +1 или -1
}

// компонент для маркера
#[derive(Component)]
pub struct AimMarker;

#[derive(Component)]
pub struct AimLineMarker;