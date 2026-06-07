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

/// Коэффициент y-sort: на сколько Z сдвигается на единицу мировой `y`.
/// Разброс по карте (~±800) даёт ±0.8 — заведомо меньше зазора между слоями.
const Y_SORT_K: f32 = 0.001;

// ---------------------------------------------------------------------------
// Граница проекции. СЕЙЧАС — тождество (top-down). Для изометрии заменить тело.
// ---------------------------------------------------------------------------

/// Мир → экран. Тождество (top-down).
///
/// Для изометрии 2:1 это станет, например:
/// ```ignore
/// Vec2::new((w.x - w.y) * 0.5 * TILE_W, (w.x + w.y) * 0.25 * TILE_W)
/// ```
#[inline]
pub fn world_to_screen(w: Vec2) -> Vec2 {
    w
}

/// Экран → мир (обратная к [`world_to_screen`]). Тождество (top-down).
#[inline]
pub fn screen_to_world(s: Vec2) -> Vec2 {
    s
}

/// Глубина (Z) для сортировки: внутри слоя объекты с меньшей мировой `y`
/// рисуются «впереди» (ближе к зрителю) — это и нужно для изометрии.
#[inline]
pub fn depth_z(world: Vec2, layer: f32) -> f32 {
    layer - world.y * Y_SORT_K
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
