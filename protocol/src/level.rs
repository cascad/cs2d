//! Общая тайловая модель уровня и коллизия движения, разделяемая клиентом и
//! сервером. Цель — чтобы предсказание клиента и авторитет сервера использовали
//! ОДИН И ТОТ ЖЕ код, иначе у стен возникает рассинхрон/дёрганье.
//!
//! Соглашение по координатам: индекс тайла = `floor(world / TILE)`, без смещения
//! origin. Это корректно, потому что origin карты выровнен по сетке тайлов
//! (кратен `TILE`), а значит глобальная нумерация тайлов согласована.

use glam::{IVec2, Vec2};
use std::collections::HashSet;

/// Мировые координаты → индекс тайла.
#[inline]
pub fn world_to_tile(p: Vec2, tile: f32) -> IVec2 {
    IVec2::new((p.x / tile).floor() as i32, (p.y / tile).floor() as i32)
}

/// Растеризует прямоугольники стен (`min`, `max`) в множество сплошных тайлов.
///
/// Каждый AABB слегка ужимается на `eps`, чтобы стена, выровненная ровно по
/// границе тайла, НЕ помечала соседний пустой тайл. Без этого стена «залезала»
/// бы в соседнюю клетку и создавала фантомную преграду шириной в тайл — именно
/// это раньше расходилось с точной коллизией сервера.
pub fn rasterize_walls(solids: &mut HashSet<IVec2>, walls: &[(Vec2, Vec2)], tile: f32) {
    let eps = Vec2::splat(0.001);
    for &(min, max) in walls {
        let t0 = world_to_tile(min + eps, tile);
        let t1 = world_to_tile(max - eps, tile);
        for tx in t0.x..=t1.x {
            for ty in t0.y..=t1.y {
                solids.insert(IVec2::new(tx, ty));
            }
        }
    }
}

/// Пересекается ли AABB игрока (центр ± `half`) хотя бы с одним сплошным тайлом.
/// O(числа тайлов под игроком) ≈ O(1), вместо перебора всех стен.
pub fn tile_blocked(solids: &HashSet<IVec2>, center: Vec2, half: f32, tile: f32) -> bool {
    let min = center - Vec2::splat(half);
    let max = center + Vec2::splat(half);
    let t0 = world_to_tile(min, tile);
    let t1 = world_to_tile(max, tile);
    for tx in t0.x..=t1.x {
        for ty in t0.y..=t1.y {
            if solids.contains(&IVec2::new(tx, ty)) {
                return true;
            }
        }
    }
    false
}

/// Один шаг движения игрока со «скольжением» вдоль стен: смещение `delta`
/// применяется отдельно по осям X и Y, каждая ось отменяется, если приводит
/// к коллизии с тайлом. Это ОБЩИЙ код предсказания клиента и авторитета сервера —
/// благодаря ему их позиции сходятся до бита при одинаковых входах.
pub fn slide_move(pos: Vec2, delta: Vec2, solids: &HashSet<IVec2>, half: f32, tile: f32) -> Vec2 {
    let mut new = pos;

    let proposed_x = Vec2::new(pos.x + delta.x, pos.y);
    if !tile_blocked(solids, proposed_x, half, tile) {
        new.x = proposed_x.x;
    }
    let proposed_y = Vec2::new(new.x, pos.y + delta.y);
    if !tile_blocked(solids, proposed_y, half, tile) {
        new.y = proposed_y.y;
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: f32 = 32.0;

    /// Стена-тайл, выровненная по сетке: занимает мировой квадрат [0,32]^2.
    fn aligned_wall() -> Vec<(Vec2, Vec2)> {
        vec![(Vec2::new(0.0, 0.0), Vec2::new(32.0, 32.0))]
    }

    #[test]
    fn world_to_tile_basic() {
        assert_eq!(world_to_tile(Vec2::new(0.0, 0.0), TILE), IVec2::new(0, 0));
        assert_eq!(world_to_tile(Vec2::new(31.9, 31.9), TILE), IVec2::new(0, 0));
        assert_eq!(world_to_tile(Vec2::new(32.0, 32.0), TILE), IVec2::new(1, 1));
        assert_eq!(world_to_tile(Vec2::new(-1.0, -1.0), TILE), IVec2::new(-1, -1));
    }

    #[test]
    fn aligned_wall_marks_exactly_one_tile() {
        // Главное свойство: стена [0,32]^2 помечает ровно тайл (0,0),
        // а не «расплывается» на соседние из-за floor границы.
        let mut solids = HashSet::new();
        rasterize_walls(&mut solids, &aligned_wall(), TILE);
        assert_eq!(solids.len(), 1);
        assert!(solids.contains(&IVec2::new(0, 0)));
    }

    #[test]
    fn player_inside_wall_tile_is_blocked() {
        let mut solids = HashSet::new();
        rasterize_walls(&mut solids, &aligned_wall(), TILE);
        // игрок (half=16) в центре тайла (16,16)
        assert!(tile_blocked(&solids, Vec2::new(16.0, 16.0), 16.0, TILE));
    }

    #[test]
    fn player_flush_against_wall_is_free() {
        let mut solids = HashSet::new();
        rasterize_walls(&mut solids, &aligned_wall(), TILE);
        // игрок справа от стены, его левый край ровно на границе x=32 (тайл 1+),
        // в сам тайл стены (0) он не заходит → свободно (нет фантомной преграды)
        assert!(!tile_blocked(&solids, Vec2::new(48.0, 16.0), 16.0, TILE));
    }

    #[test]
    fn player_far_is_free() {
        let mut solids = HashSet::new();
        rasterize_walls(&mut solids, &aligned_wall(), TILE);
        assert!(!tile_blocked(&solids, Vec2::new(500.0, 500.0), 16.0, TILE));
    }

    #[test]
    fn slide_move_free_space_moves_fully() {
        let solids = HashSet::new();
        let p = slide_move(Vec2::new(100.0, 100.0), Vec2::new(5.0, -3.0), &solids, 16.0, TILE);
        assert!((p - Vec2::new(105.0, 97.0)).length() < 1e-5);
    }

    #[test]
    fn slide_move_slides_along_wall() {
        // стена-тайл [0,32]^2; игрок слева, движется по диагонали вправо-вверх.
        // ось X упрётся в стену (отменится), ось Y проскользнёт.
        let mut solids = HashSet::new();
        rasterize_walls(&mut solids, &aligned_wall(), TILE);
        let start = Vec2::new(-17.0, 10.0); // игрок чуть левее стены, без касания
        let p = slide_move(start, Vec2::new(5.0, 5.0), &solids, 16.0, TILE);
        // по X движение заблокировано (упёрлись в стену), по Y — прошло
        assert!((p.x - start.x).abs() < 1e-5, "X должно быть заблокировано: {p:?}");
        assert!((p.y - 15.0).abs() < 1e-5, "Y должно проскользнуть: {p:?}");
    }
}
