//! Тайловые карты уровней — ЕДИНЫЙ источник для клиента и сервера. Раньше карта
//! дублировалась в `client` и `server` (риск рассинхрона коллизии). Теперь и
//! геометрия (стены/коллизия/спавны), и материалы (для отрисовки) живут здесь.
//!
//! Формат карты — сетка символов (по строке на ряд). Символы:
//!   '#' — стена (камень)        'B' — стена (кирпич, для колонн/особых залов)
//!   '.' — пол (камень)          ',' — пол (земля/грунт: подвалы, хоз. части)
//!   '=' — пол (дерево: залы со столами)   ':' — пол (плитка: атриум)
//!   '%' — пол (щебень/осыпь: разрушенные пещеры)
//!   'S' — точка спавна (пол под ней — камень)
//!   ' ' — «пустота» вне уровня (ни пол, ни стена — не рендерится)
//!
//! Координаты: origin карты центрируется в (0,0), индекс тайла = floor(world/tile)
//! — так же, как в [`crate::level`].

use crate::level::rasterize_walls;
use glam::{IVec2, Vec2};
use std::collections::HashSet;

/// Материал пола (влияет только на отрисовку, не на коллизию).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Floor {
    Stone,
    Dirt,
    Wood,
    Tiles,
    Rubble,
}

/// Материал стены (для отрисовки).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallMat {
    Stone,
    Brick,
}

/// Разбор одной клетки карты.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Блокирует движение (стена/непроходимое).
    pub solid: bool,
    /// Материал пола; `None` — «пустота» (пол не рисуем).
    pub floor: Option<Floor>,
    /// Если клетка — стена, её материал.
    pub wall: Option<WallMat>,
    /// Точка спавна игрока.
    pub spawn: bool,
}

/// Символ карты → клетка.
pub fn classify(ch: char) -> Cell {
    match ch {
        '#' => Cell { solid: true, floor: Some(Floor::Stone), wall: Some(WallMat::Stone), spawn: false },
        'B' => Cell { solid: true, floor: Some(Floor::Stone), wall: Some(WallMat::Brick), spawn: false },
        'S' => Cell { solid: false, floor: Some(Floor::Stone), wall: None, spawn: true },
        ' ' => Cell { solid: false, floor: None, wall: None, spawn: false },
        ',' => Cell { solid: false, floor: Some(Floor::Dirt), wall: None, spawn: false },
        '=' => Cell { solid: false, floor: Some(Floor::Wood), wall: None, spawn: false },
        ':' => Cell { solid: false, floor: Some(Floor::Tiles), wall: None, spawn: false },
        '%' => Cell { solid: false, floor: Some(Floor::Rubble), wall: None, spawn: false },
        // '.' и всё неизвестное — обычный каменный пол
        _ => Cell { solid: false, floor: Some(Floor::Stone), wall: None, spawn: false },
    }
}

/// Разобранный уровень: геометрия + материалы. Origin центрирует карту в (0,0).
pub struct ParsedLevel {
    pub width: usize,
    pub height: usize,
    pub tile: f32,
    pub origin: Vec2,
    /// Клетки в порядке (y по строкам, x по столбцам) с мировым центром.
    pub cells: Vec<(Vec2, Cell)>,
    /// AABB стен (min,max) — для коллизии и спатиал-сетки.
    pub wall_aabbs: Vec<(Vec2, Vec2)>,
    /// Точки спавна (мировые центры клеток 'S').
    pub spawns: Vec<Vec2>,
}

impl ParsedLevel {
    /// Множество сплошных тайлов движения (тот же растеризатор, что у сервера).
    pub fn solid_tiles(&self) -> HashSet<IVec2> {
        let mut s = HashSet::new();
        rasterize_walls(&mut s, &self.wall_aabbs, self.tile);
        s
    }
}

/// Разбор карты-сетки. Ширина = максимальная длина строки; недостающие символы
/// в коротких строках достраиваются стеной '#' (уровень остаётся замкнутым).
pub fn parse<S: AsRef<str>>(lines: &[S], tile: f32) -> ParsedLevel {
    let height = lines.len();
    let width = lines.iter().map(|l| l.as_ref().chars().count()).max().unwrap_or(0);

    let origin = Vec2::new(-(width as f32) * tile * 0.5, -(height as f32) * tile * 0.5);
    let half = Vec2::splat(tile * 0.5);

    let mut cells = Vec::with_capacity(width * height);
    let mut wall_aabbs = Vec::new();
    let mut spawns = Vec::new();

    for (jy, line) in lines.iter().enumerate() {
        let row: Vec<char> = line.as_ref().chars().collect();
        for jx in 0..width {
            let ch = row.get(jx).copied().unwrap_or('#');
            let cell = classify(ch);
            let center =
                origin + Vec2::new((jx as f32 + 0.5) * tile, (jy as f32 + 0.5) * tile);
            if cell.solid {
                wall_aabbs.push((center - half, center + half));
            }
            if cell.spawn {
                spawns.push(center);
            }
            cells.push((center, cell));
        }
    }

    ParsedLevel { width, height, tile, origin, cells, wall_aabbs, spawns }
}

// ============================================================================
// Карты
// ============================================================================

/// Старая «арена» — оставлена для совместимости/сравнения.
pub const ARENA_LEGACY: &[&str] = &[
    "##################################################",
    "#...................S...............#####........#",
    "#....................................#...........#",
    "#...#####............................#...........#",
    "#...#...#..............#####.........#...........#",
    "#...#...#............................#####.......#",
    "#...#...#........................................#",
    "#...#####........S......................#####....#",
    "#.......................................#........#",
    "#.............#####.....................#........#",
    "#.............#..........................#.......#",
    "#.............#............#####.........#####...#",
    "#.............#.................................S#",
    "#.............#####..............................#",
    "#.................................................#",
    "#....#####........................................#",
    "#....#...#........#####...........................#",
    "#....#...#........................................#",
    "#....#...#........................................#",
    "#....#####.............S................#####.....#",
    "#.........................................#.......#",
    "#.........................................#.......#",
    "#....................#####................#.......#",
    "#....................#.....................#####..#",
    "#....................#............................#",
    "#....................#####........................#",
    "#...........S.....................................#",
    "#..................................................#",
    "#....#####.........................................#",
    "#....#...#.........................................#",
    "#....#...#..........................#####.........#",
    "#....#...#..........................#.............#",
    "#....#####............#####.........#.............#",
    "#.....................#.............#.............#",
    "#.....................#.............#####.........#",
    "#.....................#...........................#",
    "#..............#####..#####........................#",
    "#..............#..................................#",
    "#..............#...............S..................#",
    "#..............#####................................",
    "#..................................................#",
    "#..................#####..........................#",
    "#..................#..............................#",
    "#..................#...........#####..............#",
    "#..................#####.......#...#..............#",
    "#..............................#...#..............#",
    "#........S.....................#...#..............#",
    "#..................................................#",
    "##################################################",
];

// ── Сетка комнат подземелья ───────────────────────────────────────────────
// Подземелье — РЕГУЛЯРНАЯ сетка комнат COLS×ROWS, разделённых ТОНКИМИ (1 тайл)
// стенами. Между каждой парой соседних комнат — дверь (проём), поэтому коридоров/
// переходов много, а «глухой» скалы — минимум (то, что просили: «стены поуже,
// коридоров побольше»). Двери центрированы по оси-центру комнат, чтобы патрульные
// маршруты вдоль центральных рядов/столбцов шли по полу через проёмы.
const ROOM_COLS: usize = 5;
const ROOM_ROWS: usize = 4;
const ROOM_W: usize = 12;
const ROOM_H: usize = 9;
const WALL: usize = 1; // толщина внутренних стен (тонкие!)
const BORDER: usize = 1;
const DUN_W: usize = BORDER * 2 + ROOM_COLS * ROOM_W + (ROOM_COLS - 1) * WALL;
const DUN_H: usize = BORDER * 2 + ROOM_ROWS * ROOM_H + (ROOM_ROWS - 1) * WALL;

/// Левый-верхний угол комнаты (col,row) в тайлах.
#[inline]
fn room_origin(col: usize, row: usize) -> (usize, usize) {
    (BORDER + col * (ROOM_W + WALL), BORDER + row * (ROOM_H + WALL))
}

/// Центр комнаты (col,row) в тайлах.
#[inline]
fn room_center_tile(col: usize, row: usize) -> (usize, usize) {
    let (x0, y0) = room_origin(col, row);
    (x0 + ROOM_W / 2, y0 + ROOM_H / 2)
}

/// Тайл (ix, iy) → мировой центр клетки (как в [`parse`], origin центрирует карту).
#[inline]
fn tile_center_world(ix: usize, iy: usize, tile: f32) -> Vec2 {
    let origin = Vec2::new(-(DUN_W as f32) * tile * 0.5, -(DUN_H as f32) * tile * 0.5);
    origin + Vec2::new((ix as f32 + 0.5) * tile, (iy as f32 + 0.5) * tile)
}

/// Материал пола комнаты — разнообразим по индексу.
fn room_floor_char(col: usize, row: usize) -> char {
    match (col + row * ROOM_COLS) % 5 {
        0 => '.', // камень
        1 => ',', // земля
        2 => '=', // дерево
        3 => ':', // плитка
        _ => '%', // щебень
    }
}

/// Процедурно собирает подземелье: сетка комнат с тонкими стенами и дверями между
/// всеми соседями (плотная сеть коридоров). Возвращает строки-сетку нашего формата.
pub fn dungeon() -> Vec<String> {
    let mut g = vec![vec!['#'; DUN_W]; DUN_H];

    let fill = |g: &mut Vec<Vec<char>>, x0: usize, y0: usize, x1: usize, y1: usize, ch: char| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if y < DUN_H && x < DUN_W {
                    g[y][x] = ch;
                }
            }
        }
    };

    // комнаты
    for r in 0..ROOM_ROWS {
        for c in 0..ROOM_COLS {
            let (x0, y0) = room_origin(c, r);
            fill(&mut g, x0, y0, x0 + ROOM_W - 1, y0 + ROOM_H - 1, room_floor_char(c, r));
        }
    }

    // двери между соседями (центрированы → патруль вдоль центра проходит)
    for r in 0..ROOM_ROWS {
        for c in 0..ROOM_COLS {
            let (cx, cy) = room_center_tile(c, r);
            // дверь вправо
            if c + 1 < ROOM_COLS {
                let wall_x = room_origin(c, r).0 + ROOM_W; // столбец стены справа
                fill(&mut g, wall_x, cy - 1, wall_x, cy + 1, '.');
            }
            // дверь вниз
            if r + 1 < ROOM_ROWS {
                let wall_y = room_origin(c, r).1 + ROOM_H; // ряд стены снизу
                fill(&mut g, cx - 1, wall_y, cx + 1, wall_y, '.');
            }
        }
    }

    // редкие колонны-«мебель» (не на центральных осях, патруль не задевают)
    for r in 0..ROOM_ROWS {
        for c in 0..ROOM_COLS {
            if (c + r) % 2 == 0 {
                let (x0, y0) = room_origin(c, r);
                g[y0 + 2][x0 + 2] = 'B';
                g[y0 + ROOM_H - 3][x0 + ROOM_W - 3] = 'B';
            }
        }
    }

    // точки спавна игроков — центры разнесённых комнат
    for &(c, r) in &[(0, 0), (4, 0), (2, 1), (0, 3), (4, 3), (2, 2)] {
        let (cx, cy) = room_center_tile(c, r);
        g[cy][cx] = 'S';
    }

    g.into_iter().map(|row| row.into_iter().collect()).collect()
}

/// Точки спавна НЕПИСЕЙ (скелетов) — центры комнат, не совпадающие со спавнами
/// игроков; гарантированно на полу (центр комнаты).
pub fn npc_spawns(tile: f32) -> Vec<Vec2> {
    [(1usize, 0usize), (3, 1), (0, 2), (4, 2), (1, 3), (3, 3), (2, 0)]
        .iter()
        .map(|&(c, r)| {
            let (cx, cy) = room_center_tile(c, r);
            tile_center_world(cx, cy, tile)
        })
        .collect()
}

/// Патрульные маршруты НЕПИСЕЙ: последовательности мировых точек (центры комнат).
/// Соседние точки — соседние комнаты с центрированной дверью, поэтому отрезок
/// между ними идёт по полу через проём (без A*-поиска). Скелет ходит по маршруту
/// «туда-обратно» (ping-pong).
pub fn patrol_routes(tile: f32) -> Vec<Vec<Vec2>> {
    let to_world = |seq: &[(usize, usize)]| -> Vec<Vec2> {
        seq.iter()
            .map(|&(c, r)| {
                let (cx, cy) = room_center_tile(c, r);
                tile_center_world(cx, cy, tile)
            })
            .collect()
    };
    vec![
        // периметр по комнатам
        to_world(&[
            (0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (4, 1), (4, 2), (4, 3),
            (3, 3), (2, 3), (1, 3), (0, 3), (0, 2), (0, 1),
        ]),
        // средние горизонтали
        to_world(&[(0, 1), (1, 1), (2, 1), (3, 1), (4, 1)]),
        to_world(&[(0, 2), (1, 2), (2, 2), (3, 2), (4, 2)]),
        // центральная вертикаль
        to_world(&[(2, 0), (2, 1), (2, 2), (2, 3)]),
    ]
}

/// Активная карта уровня — единая точка выбора для клиента и сервера.
/// Сейчас это новое подземелье; старую арену см. [`ARENA_LEGACY`].
pub fn active_level(tile: f32) -> ParsedLevel {
    parse(&dungeon(), tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: f32 = 32.0;

    #[test]
    fn dungeon_is_rectangular() {
        let d = dungeon();
        assert_eq!(d.len(), DUN_H);
        for r in &d {
            assert_eq!(r.chars().count(), DUN_W, "row: {r}");
        }
    }

    #[test]
    fn dungeon_border_is_solid() {
        let lvl = parse(&dungeon(), TILE);
        for (_, cell) in lvl.cells.iter().take(DUN_W) {
            assert!(cell.solid, "верхняя кромка должна быть стеной");
        }
        for (_, cell) in lvl.cells.iter().skip(DUN_W * (DUN_H - 1)) {
            assert!(cell.solid, "нижняя кромка должна быть стеной");
        }
    }

    #[test]
    fn dungeon_has_six_spawns_not_in_walls() {
        let lvl = parse(&dungeon(), TILE);
        assert_eq!(lvl.spawns.len(), 6);
        let solids = lvl.solid_tiles();
        for s in &lvl.spawns {
            let t = crate::level::world_to_tile(*s, TILE);
            assert!(!solids.contains(&t), "спавн не должен быть в стене: {s:?}");
        }
    }

    #[test]
    #[ignore]
    fn print_dungeon() {
        for row in dungeon() {
            println!("{row}");
        }
    }

    #[test]
    fn npc_spawns_and_patrol_waypoints_on_floor() {
        let lvl = parse(&dungeon(), TILE);
        let solids = lvl.solid_tiles();
        for s in npc_spawns(TILE) {
            let t = crate::level::world_to_tile(s, TILE);
            assert!(!solids.contains(&t), "спавн НПС не в стене: {s:?}");
        }
        for route in patrol_routes(TILE) {
            for w in route {
                let t = crate::level::world_to_tile(w, TILE);
                assert!(!solids.contains(&t), "путевая точка не в стене: {w:?}");
            }
        }
    }

    #[test]
    fn legacy_parse_matches_dimensions() {
        let lvl = parse(ARENA_LEGACY, TILE);
        assert_eq!(lvl.height, ARENA_LEGACY.len());
        assert!(!lvl.spawns.is_empty());
    }
}
