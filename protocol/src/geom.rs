//! Общая 2D-геометрия для коллизий, разделяемая клиентом и сервером.
//!
//! Важно: здесь намеренно живут ДВЕ слегка различающиеся проверки пересечения
//! отрезка с AABB, потому что в коде исторически использовались обе семантики:
//! - [`segment_aabb_intersect_t`] возвращает точку входа и считает пересечением
//!   даже стену за концом отрезка (используется в рейкастах и LOS выстрела);
//! - [`segment_aabb_intersect`] строго требует, чтобы интервал пересечения
//!   попадал в `[0, 1]` (используется в LOS гранат).
//!
//! Менять их семантику нельзя — это изменит игровое поведение.

use glam::Vec2;

/// Пересечение двух AABB (прямоугольников), заданных углами min/max.
#[inline]
pub fn aabb_intersect(min_a: Vec2, max_a: Vec2, min_b: Vec2, max_b: Vec2) -> bool {
    !(max_a.x < min_b.x || min_a.x > max_b.x || max_a.y < min_b.y || min_a.y > max_b.y)
}

/// Liang–Barsky / slab: ближайшая точка входа `t ∈ [0, 1]` отрезка `p0→p1`
/// в AABB, если пересечение есть. Стена за концом отрезка тоже считается
/// пересечением (результат при этом зажимается в `[0, 1]`).
#[inline]
pub fn segment_aabb_intersect_t(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> Option<f32> {
    let d = p1 - p0;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;

    if d.x.abs() < f32::EPSILON {
        if p0.x < min.x || p0.x > max.x {
            return None;
        }
    } else {
        let inv = 1.0 / d.x;
        let mut tmin = (min.x - p0.x) * inv;
        let mut tmax = (max.x - p0.x) * inv;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }

    if d.y.abs() < f32::EPSILON {
        if p0.y < min.y || p0.y > max.y {
            return None;
        }
    } else {
        let inv = 1.0 / d.y;
        let mut tmin = (min.y - p0.y) * inv;
        let mut tmax = (max.y - p0.y) * inv;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return None;
        }
    }

    Some(t0.clamp(0.0, 1.0))
}

/// Строгий вариант: пересекает ли отрезок `p0→p1` AABB в пределах `[0, 1]`.
/// В отличие от [`segment_aabb_intersect_t`], стена за концом отрезка
/// пересечением НЕ считается.
#[inline]
pub fn segment_aabb_intersect(p0: Vec2, p1: Vec2, min: Vec2, max: Vec2) -> bool {
    let d = p1 - p0;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;

    if d.x.abs() < f32::EPSILON {
        if p0.x < min.x || p0.x > max.x {
            return false;
        }
    } else {
        let inv_dx = 1.0 / d.x;
        let mut tmin = (min.x - p0.x) * inv_dx;
        let mut tmax = (max.x - p0.x) * inv_dx;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return false;
        }
    }

    if d.y.abs() < f32::EPSILON {
        if p0.y < min.y || p0.y > max.y {
            return false;
        }
    } else {
        let inv_dy = 1.0 / d.y;
        let mut tmin = (min.y - p0.y) * inv_dy;
        let mut tmax = (max.y - p0.y) * inv_dy;
        if tmin > tmax {
            core::mem::swap(&mut tmin, &mut tmax);
        }
        t0 = t0.max(tmin);
        t1 = t1.min(tmax);
        if t0 > t1 {
            return false;
        }
    }

    t0 <= 1.0 && t1 >= 0.0
}

/// Рейкаст по набору AABB-стен. Возвращает дистанцию до ближайшей стены
/// вдоль `dir` (нормализованного), либо `max_dist`, если попаданий нет.
pub fn raycast_aabbs(origin: Vec2, dir: Vec2, max_dist: f32, walls: &[(Vec2, Vec2)]) -> f32 {
    let end = origin + dir * max_dist;
    let mut best: Option<f32> = None;
    for &(min, max) in walls {
        if let Some(t) = segment_aabb_intersect_t(origin, end, min, max) {
            best = Some(best.map_or(t, |b| b.min(t)));
        }
    }
    best.map(|t| t * max_dist).unwrap_or(max_dist)
}

use glam::IVec2;
use std::collections::HashMap;

/// Пространственная сетка (uniform grid) AABB-стен для ускорения отрезковых
/// запросов (рейкаст / LOS) с O(N) до ~O(длины пути в ячейках).
///
/// Гарантия корректности: запросы прогоняют ровно ту же slab-проверку
/// ([`segment_aabb_intersect_t`]), что и линейный перебор, но только по стенам
/// из ячеек, которые пересекает отрезок. Стена, которую отрезок реально задевает,
/// обязательно лежит в одной из посещённых ячеек, поэтому результат идентичен
/// линейному (это закреплено property-тестами).
#[derive(Clone, Debug, Default)]
pub struct WallGrid {
    cell: f32,
    walls: Vec<(Vec2, Vec2)>,
    buckets: HashMap<IVec2, Vec<u32>>,
}

impl WallGrid {
    /// Строит сетку из AABB-стен. `cell` — размер ячейки (рекомендуется кратным
    /// размеру тайла, например 2–4 тайла).
    pub fn build(walls: &[(Vec2, Vec2)], cell: f32) -> Self {
        let cell = if cell > 0.0 { cell } else { 1.0 };
        let mut buckets: HashMap<IVec2, Vec<u32>> = HashMap::new();
        for (i, &(min, max)) in walls.iter().enumerate() {
            let c0 = world_to_cell(min, cell);
            let c1 = world_to_cell(max, cell);
            for cx in c0.x..=c1.x {
                for cy in c0.y..=c1.y {
                    buckets.entry(IVec2::new(cx, cy)).or_default().push(i as u32);
                }
            }
        }
        Self {
            cell,
            walls: walls.to_vec(),
            buckets,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.walls.is_empty()
    }

    /// Перекрывается ли круг (`center`, `radius`) хотя бы с одной стеной.
    /// Проверяются только стены из ячеек, которые задевает AABB круга, поэтому
    /// стоимость ≈ O(1) при равномерной плотности стен.
    pub fn circle_blocked(&self, center: Vec2, radius: f32) -> bool {
        if self.walls.is_empty() {
            return false;
        }
        let min = center - Vec2::splat(radius);
        let max = center + Vec2::splat(radius);
        let c0 = world_to_cell(min, self.cell);
        let c1 = world_to_cell(max, self.cell);
        let r2 = radius * radius;
        for cx in c0.x..=c1.x {
            for cy in c0.y..=c1.y {
                if let Some(idxs) = self.buckets.get(&IVec2::new(cx, cy)) {
                    for &wi in idxs {
                        let (wmin, wmax) = self.walls[wi as usize];
                        // ближайшая к центру точка прямоугольника
                        let closest = center.clamp(wmin, wmax);
                        if (center - closest).length_squared() < r2 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Шаг движения круга-игрока со скольжением вдоль стен: смещение `delta`
    /// применяется раздельно по осям X и Y, ось отменяется при коллизии.
    /// ОБЩИЙ код предсказания клиента, реплея реконсиляции и авторитета сервера —
    /// благодаря этому позиции сходятся, а у стен нет «отброса».
    pub fn slide_circle(&self, pos: Vec2, delta: Vec2, radius: f32) -> Vec2 {
        // Сначала выталкиваем из любого перекрытия со стеной: если круг хоть на
        // волос «утоплен» (например, после коррекции позиции или из-за округлений
        // float), скольжение ломалось — игрок ЗАЛИПАЛ. Депенетрация это исключает.
        let mut p = self.depenetrate(pos, radius);

        // Подшаги против туннелирования сквозь тонкие тайлы при большом delta
        // (рывок). Обычный шаг за тик мал — обычно 1 подшаг.
        let dist = delta.length();
        let steps = (dist / radius.max(1.0)).ceil().max(1.0) as i32;
        let chunk = delta / steps as f32;
        for _ in 0..steps {
            p = self.collide_and_slide(p, chunk, radius);
        }
        p
    }

    /// «Collide-and-slide»: двигаемся вдоль `delta`; упёрлись — берём НОРМАЛЬ
    /// контакта (от ближайшей точки стены к центру) и скользим по касательной;
    /// повторяем (до 3 контактов). Ключ к выпуклым углам: у вершины нормаль
    /// РАДИАЛЬНА (от точки-вершины), поэтому круг огибает угол по дуге, а не
    /// залипает (раздельное движение по осям так не умело). Узкие щели по-прежнему
    /// не пропускают — свип консервативен (в стену не входим).
    fn collide_and_slide(&self, pos: Vec2, delta: Vec2, radius: f32) -> Vec2 {
        let mut p = pos;
        let mut remaining = delta;
        for _ in 0..3 {
            if remaining.length_squared() < 1e-10 {
                break;
            }
            let (np, blocked, t) = self.sweep(p, remaining, radius);
            p = np;
            if !blocked {
                break;
            }
            let leftover = remaining * (1.0 - t);
            let n = self.contact_normal(p, radius);
            if n.length_squared() < 1e-10 {
                break;
            }
            // убираем составляющую В стену — остаётся скольжение вдоль (касательная)
            remaining = leftover - n * leftover.dot(n);
        }
        p
    }

    /// Консервативный «свип»: двигает круг вдоль `delta` максимально далеко без
    /// пересечения стен (детерминированная бисекция). Возвращает (новая позиция,
    /// упёрлись ли, доля `t` пройденного пути).
    fn sweep(&self, from: Vec2, delta: Vec2, radius: f32) -> (Vec2, bool, f32) {
        let target = from + delta;
        if !self.circle_blocked(target, radius) {
            return (target, false, 1.0);
        }
        let mut lo = 0.0f32; // свободно
        let mut hi = 1.0f32; // заблокировано
        let mut best = 0.0f32;
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            if self.circle_blocked(from + delta * mid, radius) {
                hi = mid;
            } else {
                lo = mid;
                best = mid;
            }
        }
        (from + delta * best, true, best)
    }

    /// Нормаль контакта = направление от ближайшей точки СТЕН к центру круга.
    /// Плоская грань → ось; ВЫПУКЛЫЙ угол → радиус от вершины (нужно для огибания).
    fn contact_normal(&self, center: Vec2, radius: f32) -> Vec2 {
        let probe = radius + 1.0;
        let min = center - Vec2::splat(probe);
        let max = center + Vec2::splat(probe);
        let c0 = world_to_cell(min, self.cell);
        let c1 = world_to_cell(max, self.cell);
        let mut best_d2 = f32::INFINITY;
        let mut best_n = Vec2::ZERO;
        for cx in c0.x..=c1.x {
            for cy in c0.y..=c1.y {
                let Some(idxs) = self.buckets.get(&IVec2::new(cx, cy)) else { continue };
                for &wi in idxs {
                    let (wmin, wmax) = self.walls[wi as usize];
                    let closest = center.clamp(wmin, wmax);
                    let d = center - closest;
                    let d2 = d.length_squared();
                    if d2 < best_d2 {
                        best_d2 = d2;
                        best_n = d;
                    }
                }
            }
        }
        if best_d2 > 1e-8 {
            best_n / best_d2.sqrt()
        } else {
            Vec2::ZERO
        }
    }

    /// Выталкивает круг из всех перекрывающихся стен по минимальному смещению.
    /// Детерминированно (фикс. число итераций) — одинаково на клиенте и сервере.
    fn depenetrate(&self, pos: Vec2, radius: f32) -> Vec2 {
        if self.walls.is_empty() {
            return pos;
        }
        let mut p = pos;
        for _ in 0..4 {
            let min = p - Vec2::splat(radius);
            let max = p + Vec2::splat(radius);
            let c0 = world_to_cell(min, self.cell);
            let c1 = world_to_cell(max, self.cell);
            // ищем самое глубокое перекрытие и выталкиваем из него
            let mut worst = 0.0f32;
            let mut push = Vec2::ZERO;
            for cx in c0.x..=c1.x {
                for cy in c0.y..=c1.y {
                    let Some(idxs) = self.buckets.get(&IVec2::new(cx, cy)) else { continue };
                    for &wi in idxs {
                        let (wmin, wmax) = self.walls[wi as usize];
                        let closest = p.clamp(wmin, wmax);
                        let d = p - closest;
                        let dist2 = d.length_squared();
                        if dist2 < radius * radius {
                            let dist = dist2.sqrt();
                            let pen = radius - dist;
                            if pen > worst {
                                worst = pen;
                                push = if dist > 1e-4 {
                                    // выталкиваем наружу по нормали к ближайшей точке
                                    d / dist * (pen + 1e-3)
                                } else {
                                    // центр внутри прямоугольника — выходим через
                                    // ближайшую грань (минимальное проникновение)
                                    let to_left = p.x - wmin.x;
                                    let to_right = wmax.x - p.x;
                                    let to_bot = p.y - wmin.y;
                                    let to_top = wmax.y - p.y;
                                    let m = to_left.min(to_right).min(to_bot).min(to_top);
                                    if m == to_left {
                                        Vec2::new(-(to_left + radius + 1e-3), 0.0)
                                    } else if m == to_right {
                                        Vec2::new(to_right + radius + 1e-3, 0.0)
                                    } else if m == to_bot {
                                        Vec2::new(0.0, -(to_bot + radius + 1e-3))
                                    } else {
                                        Vec2::new(0.0, to_top + radius + 1e-3)
                                    }
                                };
                            }
                        }
                    }
                }
            }
            if worst <= 0.0 {
                break;
            }
            p += push;
        }
        p
    }

    /// Дистанция до ближайшей стены вдоль `dir` (нормализованного) либо `max_dist`.
    /// Эквивалент [`raycast_aabbs`], но через сетку.
    pub fn raycast(&self, origin: Vec2, dir: Vec2, max_dist: f32) -> f32 {
        if self.walls.is_empty() {
            return max_dist;
        }
        let end = origin + dir * max_dist;
        let mut best: Option<f32> = None;
        self.for_each_cell_on_segment(origin, end, |walls, idxs| {
            for &wi in idxs {
                let (min, max) = walls[wi as usize];
                if let Some(t) = segment_aabb_intersect_t(origin, end, min, max) {
                    best = Some(best.map_or(t, |b| b.min(t)));
                }
            }
        });
        best.map(|t| t * max_dist).unwrap_or(max_dist)
    }

    /// Перекрыт ли отрезок `p0→p1` стеной (LOS). `eps` ужимает AABB стен, чтобы
    /// луч, лежащий ровно на грани, не давал ложного пересечения.
    pub fn segment_blocked(&self, p0: Vec2, p1: Vec2, eps: f32) -> bool {
        if self.walls.is_empty() {
            return false;
        }
        let e = Vec2::splat(eps);
        let mut blocked = false;
        self.for_each_cell_on_segment(p0, p1, |walls, idxs| {
            if blocked {
                return;
            }
            for &wi in idxs {
                let (min, max) = walls[wi as usize];
                if segment_aabb_intersect_t(p0, p1, min + e, max - e).is_some() {
                    blocked = true;
                    break;
                }
            }
        });
        blocked
    }

    /// Обходит все ячейки сетки, которые пересекает отрезок (Amanatides–Woo),
    /// и вызывает `f(walls, indices_in_cell)` для непустых ячеек.
    fn for_each_cell_on_segment<F: FnMut(&[(Vec2, Vec2)], &[u32])>(
        &self,
        p0: Vec2,
        p1: Vec2,
        mut f: F,
    ) {
        let c = self.cell;
        let mut cx = (p0.x / c).floor() as i32;
        let mut cy = (p0.y / c).floor() as i32;
        let ex = (p1.x / c).floor() as i32;
        let ey = (p1.y / c).floor() as i32;

        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let inf = f32::INFINITY;

        let step_x = if dx > 0.0 { 1 } else if dx < 0.0 { -1 } else { 0 };
        let step_y = if dy > 0.0 { 1 } else if dy < 0.0 { -1 } else { 0 };

        let next_bx = if step_x > 0 { (cx + 1) as f32 } else { cx as f32 } * c;
        let next_by = if step_y > 0 { (cy + 1) as f32 } else { cy as f32 } * c;

        let mut t_max_x = if step_x != 0 { (next_bx - p0.x) / dx } else { inf };
        let mut t_max_y = if step_y != 0 { (next_by - p0.y) / dy } else { inf };
        let t_delta_x = if step_x != 0 { (c / dx).abs() } else { inf };
        let t_delta_y = if step_y != 0 { (c / dy).abs() } else { inf };

        let visit = |grid: &Self, gx: i32, gy: i32, f: &mut F| {
            if let Some(idxs) = grid.buckets.get(&IVec2::new(gx, gy)) {
                f(&grid.walls, idxs);
            }
        };

        visit(self, cx, cy, &mut f);

        // защитный лимит итераций от возможных артефактов с плавающей точкой
        let max_iter = ((dx.abs() + dy.abs()) / c) as i32 + 4;
        let mut iter = 0;
        while (cx != ex || cy != ey) && iter < max_iter {
            if t_max_x < t_max_y {
                cx += step_x;
                t_max_x += t_delta_x;
            } else {
                cy += step_y;
                t_max_y += t_delta_y;
            }
            visit(self, cx, cy, &mut f);
            iter += 1;
        }
    }
}

#[inline]
fn world_to_cell(p: Vec2, cell: f32) -> IVec2 {
    IVec2::new((p.x / cell).floor() as i32, (p.y / cell).floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn aabb_overlap_touch_separate() {
        // перекрытие
        assert!(aabb_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(3.0, 3.0),
        ));
        // касание гранями считается пересечением
        assert!(aabb_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 1.0),
        ));
        // полностью раздельные
        assert!(!aabb_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(3.0, 3.0),
        ));
    }

    #[test]
    fn segment_hits_box_in_front() {
        // луч слева направо сквозь центрированную коробку [-1,1]^2
        let t = segment_aabb_intersect_t(
            Vec2::new(-5.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, 1.0),
        );
        assert!(t.is_some());
        assert!(approx(t.unwrap(), 0.4)); // вход на 40% длины отрезка
    }

    #[test]
    fn segment_misses_box() {
        // отрезок проходит выше коробки
        let t = segment_aabb_intersect_t(
            Vec2::new(-5.0, 5.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, 1.0),
        );
        assert!(t.is_none());
    }

    #[test]
    fn segment_does_not_reach_box() {
        // стена целиком за концом короткого отрезка → не пересекает
        let p0 = Vec2::new(0.0, 0.0);
        let p1 = Vec2::new(1.0, 0.0);
        let min = Vec2::new(5.0, -1.0);
        let max = Vec2::new(7.0, 1.0);
        assert!(segment_aabb_intersect_t(p0, p1, min, max).is_none());
        // строгий булев вариант даёт тот же ответ
        assert!(!segment_aabb_intersect(p0, p1, min, max));
    }

    #[test]
    fn strict_and_t_variants_agree() {
        // на интервале [0,1] обе реализации эквивалентны
        let cases = [
            (Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)),
            (Vec2::new(-5.0, 5.0), Vec2::new(5.0, 5.0)),
            (Vec2::new(0.0, -3.0), Vec2::new(0.0, 3.0)),
            (Vec2::new(-2.0, -2.0), Vec2::new(2.0, 2.0)),
        ];
        let min = Vec2::new(-1.0, -1.0);
        let max = Vec2::new(1.0, 1.0);
        for (p0, p1) in cases {
            assert_eq!(
                segment_aabb_intersect_t(p0, p1, min, max).is_some(),
                segment_aabb_intersect(p0, p1, min, max),
                "несовпадение вариантов для {:?}->{:?}",
                p0,
                p1
            );
        }
    }

    #[test]
    fn raycast_returns_distance_to_nearest_wall() {
        let origin = Vec2::new(0.0, 0.0);
        let dir = Vec2::new(1.0, 0.0);
        let walls = [
            (Vec2::new(50.0, -10.0), Vec2::new(60.0, 10.0)),
            (Vec2::new(20.0, -10.0), Vec2::new(30.0, 10.0)),
        ];
        // ближайшая стена начинается на x=20
        assert!(approx(raycast_aabbs(origin, dir, 100.0, &walls), 20.0));
    }

    #[test]
    fn raycast_no_walls_returns_max() {
        let d = raycast_aabbs(Vec2::ZERO, Vec2::new(1.0, 0.0), 100.0, &[]);
        assert!(approx(d, 100.0));
    }

    #[test]
    fn raycast_wall_beyond_range_returns_max() {
        // стена за пределами max_dist → попадания нет
        let walls = [(Vec2::new(200.0, -10.0), Vec2::new(210.0, 10.0))];
        let d = raycast_aabbs(Vec2::ZERO, Vec2::new(1.0, 0.0), 100.0, &walls);
        assert!(approx(d, 100.0));
    }

    // --- WallGrid: сетка обязана совпадать с линейным перебором ---

    /// Детерминированный ГПСЧ (xorshift64), чтобы тесты были воспроизводимы.
    struct Rng(u64);
    impl Rng {
        fn next_u32(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            (x >> 32) as u32
        }
        fn f(&mut self, lo: f32, hi: f32) -> f32 {
            let t = self.next_u32() as f32 / u32::MAX as f32;
            lo + t * (hi - lo)
        }
    }

    /// Набор стен-тайлов 32×32 в диапазоне world [-320, 320] (псевдослучайно).
    fn random_walls(rng: &mut Rng, n: usize) -> Vec<(Vec2, Vec2)> {
        let mut walls = Vec::new();
        for _ in 0..n {
            let tx = rng.f(-10.0, 10.0).floor();
            let ty = rng.f(-10.0, 10.0).floor();
            let min = Vec2::new(tx * 32.0, ty * 32.0);
            walls.push((min, min + Vec2::splat(32.0)));
        }
        walls
    }

    /// Линейный эталон LOS (как в боевом коде до сетки).
    fn linear_los(p0: Vec2, p1: Vec2, walls: &[(Vec2, Vec2)], eps: f32) -> bool {
        let e = Vec2::splat(eps);
        walls
            .iter()
            .any(|&(min, max)| segment_aabb_intersect_t(p0, p1, min + e, max - e).is_some())
    }

    #[test]
    fn grid_raycast_matches_linear() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for _ in 0..50 {
            let walls = random_walls(&mut rng, 40);
            let grid = WallGrid::build(&walls, 64.0);
            for _ in 0..200 {
                let origin = Vec2::new(rng.f(-350.0, 350.0), rng.f(-350.0, 350.0));
                let ang = rng.f(0.0, std::f32::consts::TAU);
                let dir = Vec2::new(ang.cos(), ang.sin());
                let max_dist = rng.f(10.0, 900.0);
                let a = raycast_aabbs(origin, dir, max_dist, &walls);
                let b = grid.raycast(origin, dir, max_dist);
                assert!(
                    (a - b).abs() < 0.05,
                    "raycast mismatch: linear={a} grid={b} origin={origin:?} dir={dir:?}"
                );
            }
        }
    }

    #[test]
    fn grid_segment_blocked_matches_linear() {
        let mut rng = Rng(0xD1B54A32D192ED03);
        let eps = 0.001;
        let mut mismatches = 0;
        for _ in 0..50 {
            let walls = random_walls(&mut rng, 40);
            let grid = WallGrid::build(&walls, 64.0);
            for _ in 0..200 {
                let p0 = Vec2::new(rng.f(-350.0, 350.0), rng.f(-350.0, 350.0));
                let p1 = Vec2::new(rng.f(-350.0, 350.0), rng.f(-350.0, 350.0));
                let a = linear_los(p0, p1, &walls, eps);
                let b = grid.segment_blocked(p0, p1, eps);
                if a != b {
                    mismatches += 1;
                }
            }
        }
        assert_eq!(mismatches, 0, "grid LOS разошёлся с линейным {mismatches} раз");
    }

    #[test]
    fn grid_empty_behaves_like_no_walls() {
        let grid = WallGrid::build(&[], 64.0);
        assert!(grid.is_empty());
        assert!(approx(grid.raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 100.0), 100.0));
        assert!(!grid.segment_blocked(Vec2::ZERO, Vec2::new(100.0, 0.0), 0.001));
    }

    // --- круговая коллизия движения ---

    #[test]
    fn circle_blocked_overlap_and_clearance() {
        // стена-квадрат [0,32]^2
        let grid = WallGrid::build(&[(Vec2::ZERO, Vec2::splat(32.0))], 64.0);
        // центр радиуса 16 на x=40 → ближайшая точка (32,16), dist=8 < 16 → задет
        assert!(grid.circle_blocked(Vec2::new(40.0, 16.0), 16.0));
        // центр на x=49 → dist=17 > 16 → свободно
        assert!(!grid.circle_blocked(Vec2::new(49.0, 16.0), 16.0));
    }

    #[test]
    fn circle_fits_through_gap_1_5x_its_size() {
        // вертикальный коридор: левая стена [0,32]x[0,400], правая [80,112]x[0,400].
        // щель по миру [32,80] = 48px = 1.5 * размер игрока (32). Радиус 16.
        let walls = [
            (Vec2::new(0.0, 0.0), Vec2::new(32.0, 400.0)),
            (Vec2::new(80.0, 0.0), Vec2::new(112.0, 400.0)),
        ];
        let grid = WallGrid::build(&walls, 64.0);
        let r = 16.0;
        // по центру щели (x=56) игрок свободно проходит по всей высоте
        let mut pos = Vec2::new(56.0, 10.0);
        for _ in 0..80 {
            pos = grid.slide_circle(pos, Vec2::new(0.0, 4.5), r);
        }
        assert!(pos.y > 200.0, "игрок должен пройти сквозь щель 1.5x: {pos:?}");
        assert!((pos.x - 56.0).abs() < 1e-3, "X не должен дёргаться: {pos:?}");
    }

    #[test]
    fn circle_does_not_fit_through_narrow_gap() {
        // щель уже диаметра (28px < 32) — игрок не пролезает по центру,
        // но и не телепортируется сквозь.
        let walls = [
            (Vec2::new(0.0, 0.0), Vec2::new(32.0, 400.0)),
            (Vec2::new(60.0, 0.0), Vec2::new(92.0, 400.0)),
        ];
        let grid = WallGrid::build(&walls, 64.0);
        let r = 16.0;
        // стартуем перед щелью и пытаемся пройти
        let mut pos = Vec2::new(46.0, -40.0);
        for _ in 0..40 {
            pos = grid.slide_circle(pos, Vec2::new(0.0, 4.5), r);
        }
        // упёрся в стену перед щелью, не прошёл далеко
        assert!(pos.y < 10.0, "узкая щель не должна пропускать: {pos:?}");
    }

    #[test]
    fn overlapping_circle_can_still_slide_along_wall() {
        // Круг чуть «утоплен» в вертикальную стену (x∈[100,150]) и движется ВДОЛЬ
        // неё (+Y) с лёгкой составляющей В стену (+X). Не должен залипать.
        let grid = WallGrid::build(&[(Vec2::new(100.0, 0.0), Vec2::new(150.0, 400.0))], 64.0);
        let r = 16.0;
        // центр на x=85 → ближайшая точка (100,*), dist=15 < 16 → перекрытие
        let mut pos = Vec2::new(85.0, 50.0);
        for _ in 0..40 {
            pos = grid.slide_circle(pos, Vec2::new(2.0, 5.0), r);
        }
        assert!(pos.y > 150.0, "должен скользить вверх вдоль стены, а не залипнуть: {pos:?}");
        assert!(pos.x + r <= 100.0 + 1e-2, "не должен оставаться внутри стены: {pos:?}");
    }

    #[test]
    fn slides_around_convex_corner_not_stuck() {
        // Один тайл-стена [0,32]^2; её НИЖНЕ-ЛЕВЫЙ угол (0,0) — выпуклый, торчит к
        // игроку из квадранта (−,−). Игрок подходит к углу почти по диагонали, но
        // чуть в сторону (не строго в вершину) — должен ОБОГНУТЬ угол, а не залипнуть.
        let grid = WallGrid::build(&[(Vec2::new(0.0, 0.0), Vec2::splat(32.0))], 64.0);
        let r = 16.0;
        let mut pos = Vec2::new(-30.0, -10.0);
        // движение вверх-вправо со смещением: преобладает +X (вдоль нижней грани)
        for _ in 0..80 {
            pos = grid.slide_circle(pos, Vec2::new(5.0, 2.0), r);
        }
        // должен проскользить мимо угла далеко вправо, а не застрять у вершины
        assert!(pos.x > 60.0, "должен обогнуть выпуклый угол и уйти вправо: {pos:?}");
    }

    #[test]
    fn slide_circle_slides_along_wall_no_throwback() {
        // движение по диагонали в стену слева: X упирается, Y скользит.
        let grid = WallGrid::build(&[(Vec2::new(0.0, 0.0), Vec2::new(32.0, 200.0))], 64.0);
        let r = 16.0;
        let start = Vec2::new(-17.0, 10.0); // вплотную слева, без перекрытия
        let p = grid.slide_circle(start, Vec2::new(5.0, 5.0), r);
        // X не пробивает стену (правый край ≤ 0) и не отбрасывает назад, но может
        // «прижаться» вплотную (до x≈-16); Y свободно скользит.
        assert!(p.x + r <= 1e-3, "X не должен пробивать стену: {p:?}");
        assert!(p.x >= start.x - 1e-3, "X не должен отбрасывать назад: {p:?}");
        assert!((p.y - 15.0).abs() < 1e-3, "Y должно скользить: {p:?}");
    }
}
