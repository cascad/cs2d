//! Полноэкранная «вспышка урона»: когда по локальному игроку проходит РЕАЛЬНЫЙ
//! урон (как в шутерах), весь экран на миг подкрашивается красным по краям и
//! быстро гаснет. По блоку (урон = 0) НЕ подсвечиваем — урон не прошёл.

use bevy::prelude::*;

use crate::events::PlayerDamagedEvent;
use crate::resources::MyPlayer;

/// Максимальная непрозрачность вспышки на полный «смачный» удар.
const FLASH_MAX_ALPHA: f32 = 0.42;
/// Каким уроном вспышка достигает почти максимума (для нормировки силы).
const FLASH_REF_DAMAGE: f32 = 40.0;
/// Скорость угасания (доля «оставшейся» интенсивности в секунду): больше — резче.
const FLASH_DECAY_PER_SEC: f32 = 6.0;
/// Базовый красный цвет вспышки.
const FLASH_RGB: (f32, f32, f32) = (0.75, 0.0, 0.0);

/// Текущая интенсивность вспышки (0..1), живёт на оверлей-ноде.
#[derive(Component, Default)]
pub struct DamageFlash {
    intensity: f32,
}

/// Полноэкранный оверлей на весь экран поверх HUD. Радиальная «виньетка» в Bevy
/// UI без шейдера недоступна, поэтому используем сплошную заливку с лёгкой
/// прозрачностью — читается как «экран мигнул красным».
pub fn setup_damage_flash(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(FLASH_RGB.0, FLASH_RGB.1, FLASH_RGB.2, 0.0)),
        // поверх остального HUD; без Interaction-компонента клики не перехватываются
        GlobalZIndex(50),
        DamageFlash::default(),
    ));
}

/// Поднимает интенсивность по событию урона (только локальный игрок и только если
/// урон > 0) и плавно гасит её каждый кадр, записывая в альфу фона.
pub fn update_damage_flash(
    time: Res<Time>,
    my: Res<MyPlayer>,
    mut ev: MessageReader<PlayerDamagedEvent>,
    mut q: Query<(&mut DamageFlash, &mut BackgroundColor)>,
) {
    let mut hit = 0.0f32;
    for e in ev.read() {
        // блок = 0 урона → не мигаем; чужой урон тоже игнорируем
        if e.id == my.id && e.damage > 0 {
            let k = (e.damage as f32 / FLASH_REF_DAMAGE).clamp(0.25, 1.0);
            hit = hit.max(k);
        }
    }

    let dt = time.delta_secs();
    for (mut flash, mut bg) in q.iter_mut() {
        if hit > flash.intensity {
            flash.intensity = hit;
        }
        // экспоненциальное угасание (кадронезависимое)
        flash.intensity *= (-FLASH_DECAY_PER_SEC * dt).exp();
        if flash.intensity < 0.003 {
            flash.intensity = 0.0;
        }
        bg.0 = Color::srgba(
            FLASH_RGB.0,
            FLASH_RGB.1,
            FLASH_RGB.2,
            flash.intensity * FLASH_MAX_ALPHA,
        );
    }
}
