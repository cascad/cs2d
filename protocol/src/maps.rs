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
//! Декор-пропы (бочки/сундуки/колонны) живут ОТДЕЛЬНЫМ списком ([`dungeon_props`]),
//! а не символами карты: так пол под пропом остаётся «родным» полом комнаты, а
//! коллизия пропа добавляется к стенам как полноразмерный тайл (см. [`active_level`]).
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

/// Декор-проп (стоит на полу, БЛОКИРУЕТ движение как полноразмерный тайл).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    Barrel,
    Barrels,
    BarrelsStacked,
    Chest,
    Column,
}

impl Prop {
    /// Блокирует ли проп ЛИНИЮ ВЗГЛЯДА (LOS). Все пропы блокируют ДВИЖЕНИЕ, но
    /// низкие — бочки/сундуки — «просвечивают»: за ними видно врага. Высокие —
    /// колонны — глухие, как стена. Источник правды для серверного куллинга и
    /// клиентского тумана (одинаково).
    #[inline]
    pub fn blocks_vision(self) -> bool {
        matches!(self, Prop::Column)
    }
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
    /// AABB стен (min,max) — для коллизии ДВИЖЕНИЯ и спатиал-сетки. Включает ВСЕ
    /// пропы (они блокируют движение как полноразмерный тайл).
    pub wall_aabbs: Vec<(Vec2, Vec2)>,
    /// AABB препятствий, БЛОКИРУЮЩИХ ВЗГЛЯД (LOS): стены + только «глухие» пропы
    /// (колонны). Низкие пропы (бочки/сундуки) сюда НЕ входят — за ними видно.
    /// Используется для серверного куллинга снапшота и клиентского тумана.
    pub vision_aabbs: Vec<(Vec2, Vec2)>,
    /// Точки спавна (мировые центры клеток 'S').
    pub spawns: Vec<Vec2>,
    /// Декор-пропы: мировой центр клетки + вид пропа (для отрисовки на клиенте).
    pub props: Vec<(Vec2, Prop)>,
}

impl ParsedLevel {
    /// Множество сплошных тайлов движения (тот же растеризатор, что у сервера).
    pub fn solid_tiles(&self) -> HashSet<IVec2> {
        let mut s = HashSet::new();
        rasterize_walls(&mut s, &self.wall_aabbs, self.tile);
        s
    }

    /// Мировой центр клетки (ix, iy) для этой карты (как в [`parse`]).
    #[inline]
    pub fn tile_center(&self, ix: usize, iy: usize) -> Vec2 {
        self.origin + Vec2::new((ix as f32 + 0.5) * self.tile, (iy as f32 + 0.5) * self.tile)
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

    // На этапе разбора карты сплошные клетки — это только стены, поэтому LOS-стены
    // совпадают с коллизионными. Пропы (и их «глухость») добавит `active_level`.
    let vision_aabbs = wall_aabbs.clone();
    ParsedLevel { width, height, tile, origin, cells, wall_aabbs, vision_aabbs, spawns, props: Vec::new() }
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

// ── Ручная карта подземелья (ala Diablo) ──────────────────────────────────
// Карта НЕ процедурная: 9 комнат РАЗНЫХ размеров с РАЗНЫМИ полами, соединённых
// широкими (3 тайла) коридорами с дверными проёмами; между комнатами — ТОЛСТЫЕ
// каменные стены (а не тонкие «заборчики», как было), поэтому стены читаются как
// границы залов. Декор (колонны/бочки/сундуки) добавляет «обжитости».
//
// Карта строится вырезанием комнат/коридоров в сплошной скале — так гарантируется
// прямоугольность и замкнутость без ручного подсчёта символов.

const MAP_W: usize = 52;
const MAP_H: usize = 40;

/// Комната: прямоугольник [x0,x1]×[y0,y1] (включительно) с символом пола.
struct Room {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    floor: char,
}

/// 9 комнат сетки 3×3 (индексы: 0=TL 1=TC 2=TR 3=ML 4=C 5=MR 6=BL 7=BC 8=BR).
const ROOMS: [Room; 9] = [
    Room { x0: 2, y0: 2, x1: 14, y1: 10, floor: ':' },   // 0 TL — плитка
    Room { x0: 20, y0: 2, x1: 31, y1: 9, floor: '.' },   // 1 TC — камень
    Room { x0: 37, y0: 2, x1: 49, y1: 11, floor: '=' },  // 2 TR — дерево
    Room { x0: 2, y0: 15, x1: 14, y1: 25, floor: ',' },  // 3 ML — земля
    Room { x0: 20, y0: 14, x1: 33, y1: 26, floor: '.' }, // 4 C  — большой зал, камень
    Room { x0: 39, y0: 15, x1: 49, y1: 26, floor: '%' }, // 5 MR — щебень
    Room { x0: 2, y0: 29, x1: 15, y1: 37, floor: '%' },  // 6 BL — щебень
    Room { x0: 20, y0: 30, x1: 33, y1: 37, floor: ':' }, // 7 BC — плитка
    Room { x0: 37, y0: 29, x1: 49, y1: 37, floor: ',' }, // 8 BR — земля
];

/// Центры комнат в тайлах (для спавнов НПС и путевых точек патруля). Эти линии
/// (ряды 6/20/33 и столбцы 8/26/43-44) держим СВОБОДНЫМИ от пропов, чтобы
/// прямые отрезки маршрута шли по полу через проёмы.
const ROOM_CENTERS: [(usize, usize); 9] = [
    (8, 6),   // 0 TL
    (25, 6),  // 1 TC
    (43, 6),  // 2 TR
    (8, 20),  // 3 ML
    (26, 20), // 4 C
    (44, 20), // 5 MR
    (8, 33),  // 6 BL
    (26, 33), // 7 BC
    (43, 33), // 8 BR
];

/// Коридоры: (x0,y0,x1,y1) прямоугольники-полосы (3 тайла шириной), вырезаемые
/// камнем-полом. Каждый соединяет пару соседних комнат проёмом по их центрам.
const CORRIDORS: [(usize, usize, usize, usize); 12] = [
    (15, 5, 19, 7),   // TL-TC
    (32, 5, 36, 7),   // TC-TR
    (6, 11, 8, 14),   // TL-ML
    (25, 10, 27, 13), // TC-C
    (43, 12, 45, 14), // TR-MR
    (15, 19, 19, 21), // ML-C
    (34, 19, 38, 21), // C-MR
    (6, 26, 8, 28),   // ML-BL
    (25, 27, 27, 29), // C-BC
    (43, 27, 45, 28), // MR-BR
    (16, 33, 19, 35), // BL-BC
    (34, 33, 36, 35), // BC-BR
];

/// Точки спавна игроков (тайлы) — по углам разнесённых комнат (не в центрах).
const PLAYER_SPAWNS: [(usize, usize); 6] =
    [(4, 8), (47, 4), (4, 23), (47, 17), (4, 35), (47, 35)];

/// Декор-пропы карты: (тайл x, тайл y, вид). Держатся ВНЕ центральных линий
/// маршрутов и вне спавнов (см. комментарии к [`ROOM_CENTERS`]).
fn props_layout() -> Vec<(usize, usize, Prop)> {
    use Prop::*;
    vec![
        // центральный зал C: колонны по углам + сундук и бочки
        (22, 16, Column),
        (31, 16, Column),
        (22, 24, Column),
        (31, 24, Column),
        (23, 18, Chest),
        (29, 17, BarrelsStacked),
        (24, 23, Barrel),
        // TL
        (12, 3, Barrel),
        (3, 9, BarrelsStacked),
        // TC
        (21, 3, Barrel),
        (30, 8, Barrels),
        // TR
        (38, 10, Chest),
        (48, 10, Barrels),
        // ML
        (12, 16, Barrel),
        (3, 24, BarrelsStacked),
        // MR
        (40, 16, Barrels),
        (48, 25, Barrel),
        // BL
        (3, 30, Barrel),
        (14, 36, Chest),
        // BC
        (22, 31, Column),
        (31, 31, Column),
        (24, 36, Barrel),
        // BR
        (38, 30, Barrels),
        (48, 30, Barrel),
    ]
}

/// Собирает ручную карту: вырезает комнаты и коридоры в сплошной скале, ставит
/// спавны. Возвращает строки-сетку нашего формата (ровно `MAP_W`×`MAP_H`).
pub fn dungeon() -> Vec<String> {
    let mut g = vec![vec!['#'; MAP_W]; MAP_H];

    let fill = |g: &mut Vec<Vec<char>>, x0: usize, y0: usize, x1: usize, y1: usize, ch: char| {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if y < MAP_H && x < MAP_W {
                    g[y][x] = ch;
                }
            }
        }
    };

    // комнаты (свой пол у каждой)
    for r in &ROOMS {
        fill(&mut g, r.x0, r.y0, r.x1, r.y1, r.floor);
    }
    // коридоры (каменный пол)
    for &(x0, y0, x1, y1) in &CORRIDORS {
        fill(&mut g, x0, y0, x1, y1, '.');
    }
    // спавны игроков
    for &(x, y) in &PLAYER_SPAWNS {
        g[y][x] = 'S';
    }

    g.into_iter().map(|row| row.into_iter().collect()).collect()
}

/// Пропы карты (тайл x, y, вид) — отдельно от символьной сетки, чтобы пол под
/// пропом оставался «родным» полом комнаты.
pub fn dungeon_props() -> Vec<(usize, usize, Prop)> {
    props_layout()
}

/// Точки спавна НЕПИСЕЙ (скелетов/зомби) — центры комнат; гарантированно на полу.
pub fn npc_spawns(tile: f32) -> Vec<Vec2> {
    // подмножество центров комнат (центры не совпадают со спавнами игроков —
    // те по углам).
    [1usize, 4, 7, 3, 5, 0, 8]
        .iter()
        .map(|&i| {
            let (cx, cy) = ROOM_CENTERS[i];
            tile_to_world(cx, cy, tile)
        })
        .collect()
}

/// Патрульные маршруты НЕПИСЕЙ: последовательности мировых точек (центры комнат).
/// Соседние точки — соседние комнаты, соединённые коридором по центральной оси,
/// поэтому прямой отрезок между ними идёт по полу через проём (без A*-поиска).
pub fn patrol_routes(tile: f32) -> Vec<Vec<Vec2>> {
    let to_world = |seq: &[usize]| -> Vec<Vec2> {
        seq.iter()
            .map(|&i| {
                let (cx, cy) = ROOM_CENTERS[i];
                tile_to_world(cx, cy, tile)
            })
            .collect()
    };
    vec![
        // периметр по комнатам: TL→TC→TR→MR→BR→BC→BL→ML
        to_world(&[0, 1, 2, 5, 8, 7, 6, 3]),
        // вертикаль через центр: TC→C→BC
        to_world(&[1, 4, 7]),
        // горизонталь через центр: ML→C→MR
        to_world(&[3, 4, 5]),
        // диагональная связка через центр: TL→ML→C→MR→TR
        to_world(&[0, 3, 4, 5, 2]),
    ]
}

/// Тайл (ix, iy) → мировой центр клетки активной карты (origin центрирует карту).
#[inline]
fn tile_to_world(ix: usize, iy: usize, tile: f32) -> Vec2 {
    let origin = Vec2::new(-(MAP_W as f32) * tile * 0.5, -(MAP_H as f32) * tile * 0.5);
    origin + Vec2::new((ix as f32 + 0.5) * tile, (iy as f32 + 0.5) * tile)
}

/// Активная карта уровня — единая точка выбора для клиента и сервера.
/// Пропы добавляются как полноразмерные тайлы-коллизии (в `wall_aabbs`) и как
/// список для отрисовки (`props`); старую арену см. [`ARENA_LEGACY`].
pub fn active_level(tile: f32) -> ParsedLevel {
    let mut lvl = parse(&dungeon(), tile);
    let half = Vec2::splat(tile * 0.5);
    for (ix, iy, prop) in dungeon_props() {
        let center = lvl.tile_center(ix, iy);
        // движение блокируют ВСЕ пропы…
        lvl.wall_aabbs.push((center - half, center + half));
        // …а взгляд — только глухие (колонны). Бочки/сундуки «просвечивают».
        if prop.blocks_vision() {
            lvl.vision_aabbs.push((center - half, center + half));
        }
        lvl.props.push((center, prop));
    }
    lvl
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: f32 = 32.0;

    #[test]
    fn dungeon_is_rectangular() {
        let d = dungeon();
        assert_eq!(d.len(), MAP_H);
        for r in &d {
            assert_eq!(r.chars().count(), MAP_W, "row: {r}");
        }
    }

    #[test]
    fn dungeon_border_is_solid() {
        let lvl = parse(&dungeon(), TILE);
        for (_, cell) in lvl.cells.iter().take(MAP_W) {
            assert!(cell.solid, "верхняя кромка должна быть стеной");
        }
        for (_, cell) in lvl.cells.iter().skip(MAP_W * (MAP_H - 1)) {
            assert!(cell.solid, "нижняя кромка должна быть стеной");
        }
    }

    #[test]
    fn dungeon_has_six_spawns_not_in_walls() {
        let lvl = active_level(TILE);
        assert_eq!(lvl.spawns.len(), 6);
        let solids = lvl.solid_tiles();
        for s in &lvl.spawns {
            let t = crate::level::world_to_tile(*s, TILE);
            assert!(!solids.contains(&t), "спавн не должен быть в стене/пропе: {s:?}");
        }
    }

    #[test]
    fn props_are_on_floor_and_block() {
        // каждый проп стоит на полу (не в стене) и добавляет коллизию-тайл.
        let lvl = active_level(TILE);
        assert!(!lvl.props.is_empty());
        let base = parse(&dungeon(), TILE);
        let base_solids = base.solid_tiles();
        for (pos, _) in &lvl.props {
            let t = crate::level::world_to_tile(*pos, TILE);
            assert!(!base_solids.contains(&t), "проп не должен стоять в стене: {pos:?}");
        }
        // с пропами сплошных тайлов БОЛЬШЕ, чем без них
        assert!(lvl.solid_tiles().len() > base_solids.len());
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
        // важно: путевые точки и спавны НПС не должны попадать в стены/пропы,
        // иначе скелет «родится в стене». Проверяем по карте С пропами.
        let lvl = active_level(TILE);
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
