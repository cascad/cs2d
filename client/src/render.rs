//! Граница «мир ↔ экран» и единый источник правды позиции для отрисовки.
//!
//! Идея подготовки к изометрии (ala Diablo): вся игровая логика и сеть живут в
//! МИРОВЫХ координатах (плоскость X/Y). Сущности хранят свою мировую позицию в
//! компоненте [`WorldPos`]. Ровно одна система [`project_world_to_transform`]
//! раз в кадр переводит `WorldPos` в `Transform` (экранные координаты) через
//! функции [`world_to_screen`]/[`depth_z`].
//!
//! Сейчас проекция — ТОЖДЕСТВО (top-down), поэтому визуал не меняется. Чтобы
//! включить изометрию, достаточно поменять тело [`world_to_screen`] /
//! [`screen_to_world`] (и при желании [`depth_z`]) — все системы, которые ходят
//! через эту границу, продолжат работать без изменений.

use bevy::prelude::*;

/// Авторитетная позиция сущности в мировых координатах (плоскость X/Y).
/// Источник правды для отрисовки: `Transform` вычисляется из неё проекцией.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WorldPos(pub Vec2);

/// Базовый «слой» отрисовки (грубая Z-сортировка между категориями объектов).
/// Точная сортировка внутри слоя добавляется по `WorldPos.y` (y-sort).
#[derive(Component, Clone, Copy, Debug)]
pub struct RenderLayer(pub f32);

impl Default for RenderLayer {
    fn default() -> Self {
        Self(layers::ACTOR)
    }
}

/// Базовые Z слоёв. Между соседними слоями зазор много больше, чем разброс
/// y-sort (см. [`depth_z`]), поэтому слои не «перетекают» друг в друга.
#[allow(dead_code)] // полная схема слоёв задокументирована; не все ещё задействованы
pub mod layers {
    pub const FLOOR: f32 = 0.0;
    pub const WALL: f32 = 100.0;
    pub const CORPSE: f32 = 150.0;
    pub const ACTOR: f32 = 200.0;
    /// Туман войны: затемняет всё ниже себя (пол/стены/трупы/актёры), но НЕ
    /// прицел/эффекты (они выше) — выстрелы/взрывы остаются видимы как «подсказка».
    pub const FOG: f32 = 250.0;
    pub const AIM: f32 = 300.0;
    pub const EFFECT: f32 = 400.0;
    pub const PROJECTILE: f32 = 450.0;
}

/// Коэффициент глубины: на сколько Z сдвигается на единицу мировой «дальности»
/// (`world.x + world.y`). Разброс по карте (~±1600) даёт ±1.6 — заведомо меньше
/// зазора между слоями (100), поэтому слои не перетекают.
const Y_SORT_K: f32 = 0.001;

// ---------------------------------------------------------------------------
// Граница проекции — ИЗОМЕТРИЯ 2:1 (ala Diablo).
//
// Мир остаётся плоскостью X/Y (вся логика/сеть/коллизии — в мировых
// координатах). Здесь — ЛИНЕЙНОЕ отображение мир→экран через начало координат,
// поэтому одной и той же функцией проецируются и точки, и направления (для
// направления просто не добавляется смещение, а его нет).
// ---------------------------------------------------------------------------

/// Полуширина проекции на единицу мира по экранному X.
// Единый источник изо-констант — в `protocol::constants` (их же использует
// расчёт анизотропной скорости, чтобы экранные направления совпадали).
pub use protocol::constants::{ISO_X, ISO_Y};

/// Мир → экран (изометрия 2:1). `+world.y` («север») уходит вверх-вправо,
/// `+world.x` («восток») — вниз-вправо. Линейна (через 0,0).
#[inline]
pub fn world_to_screen(w: Vec2) -> Vec2 {
    Vec2::new((w.x - w.y) * ISO_X, (w.x + w.y) * ISO_Y)
}

/// Экран → мир (обратная к [`world_to_screen`]).
#[inline]
pub fn screen_to_world(s: Vec2) -> Vec2 {
    let a = s.x / ISO_X; // = world.x - world.y
    let b = s.y / ISO_Y; // = world.x + world.y
    Vec2::new((a + b) * 0.5, (b - a) * 0.5)
}

/// Глубина (Z) для сортировки внутри слоя: объекты «ближе к камере» (меньше
/// `world.x + world.y`, т.е. ниже на экране) рисуются поверх дальних.
#[inline]
pub fn depth_z(world: Vec2, layer: f32) -> f32 {
    layer - (world.x + world.y) * Y_SORT_K
}

/// Полный перевод мировой позиции в `Transform.translation` (вкл. Z).
#[inline]
pub fn world_to_translation(world: Vec2, layer: f32) -> Vec3 {
    world_to_screen(world).extend(depth_z(world, layer))
}

// ---------------------------------------------------------------------------
// Система проекции и хелпер курсора
// ---------------------------------------------------------------------------

/// Единственная система, которая пишет `Transform.translation` из `WorldPos`.
/// Вращение (`Transform.rotation`) она НЕ трогает — его задают системы прицела/
/// интерполяции/анимаций. Должна идти после всех, кто двигает `WorldPos`, и до
/// камеры (которая читает `Transform`).
pub fn project_world_to_transform(
    mut q: Query<(&WorldPos, Option<&RenderLayer>, &mut Transform)>,
) {
    for (wp, layer, mut tf) in q.iter_mut() {
        let l = layer.map(|x| x.0).unwrap_or(layers::ACTOR);
        let t = world_to_translation(wp.0, l);
        tf.translation.x = t.x;
        tf.translation.y = t.y;
        tf.translation.z = t.z;
    }
}

/// Единая точка «курсор → мир». Возвращает мировую позицию под курсором с
/// учётом камеры и обратной проекции (для изометрии — через [`screen_to_world`]).
pub fn pointer_world(window: &Window, camera: &Camera, cam_tf: &GlobalTransform) -> Option<Vec2> {
    let cursor = window.cursor_position()?;
    let screen = camera.viewport_to_world_2d(cam_tf, cursor).ok()?;
    Some(screen_to_world(screen))
}
