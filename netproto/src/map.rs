//! Общая карта для Lightyear-стека: те же стены/обзор/спавны, что и в старом
//! стеке (из `protocol::maps`). Сетки строятся одним кодом на клиенте и сервере,
//! поэтому коллизия и LOS совпадают до бита (нужно для предсказания без рассинхрона).

use bevy::prelude::*;
use protocol::geom::WallGrid;
use protocol::maps;

/// Размер тайла мира (как в старом `level_fixed.rs`).
pub const TILE: f32 = 32.0;
/// Размер ячейки спатиал-сетки стен (кратен тайлу).
const WALL_GRID_CELL: f32 = TILE * 2.0;

/// Геометрия уровня как ресурс: сетка движения (все стены/пропы), сетка обзора
/// (без низких пропов — для тумана/LOS) и точки спавна.
#[derive(Resource, Clone)]
pub struct MapGrids {
    /// Коллизия движения (скольжение круга игрока/НИП вдоль стен).
    pub movement: WallGrid,
    /// Блокировка взгляда (LOS) — для тумана войны и видимости снапшота.
    pub vision: WallGrid,
    /// Сырые AABB стен движения — для физики гранат (`protocol::grenade`).
    pub wall_aabbs: Vec<(Vec2, Vec2)>,
    /// Точки спавна игроков (мировые центры клеток 'S').
    pub spawns: Vec<Vec2>,
    /// «Углы» карты для респауна неписей: проходимая клетка с полом, ближайшая
    /// к каждому углу bbox карты. Непись возрождается в дальнем от игроков углу
    /// и идёт к своему «дому» — а не материализуется посреди комнаты за спиной.
    pub npc_corners: Vec<Vec2>,
}

impl MapGrids {
    /// Строит карту из общего описания (`protocol::maps::active_level`).
    pub fn load() -> Self {
        let lvl = maps::active_level(TILE);
        let movement = WallGrid::build(&lvl.wall_aabbs, WALL_GRID_CELL);
        let vision = WallGrid::build(&lvl.vision_aabbs, WALL_GRID_CELL);

        // Углы: для каждого угла bbox — ближайшая клетка, где реально можно
        // стоять (есть пол и не стена/проп; «пустота» вне комнат не годится).
        let half = Vec2::new(
            lvl.width as f32 * TILE * 0.5,
            lvl.height as f32 * TILE * 0.5,
        );
        let corners = [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, half.y),
        ];
        let npc_corners = corners
            .iter()
            .filter_map(|corner| {
                lvl.cells
                    .iter()
                    .filter(|(_, c)| !c.solid && c.floor.is_some())
                    .min_by(|(a, _), (b, _)| {
                        a.distance_squared(*corner)
                            .total_cmp(&b.distance_squared(*corner))
                    })
                    .map(|(pos, _)| *pos)
            })
            .collect();

        Self {
            movement,
            vision,
            wall_aabbs: lvl.wall_aabbs,
            spawns: lvl.spawns,
            npc_corners,
        }
    }
}

impl Default for MapGrids {
    fn default() -> Self {
        Self::load()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npc_corners_cover_four_quadrants() {
        let m = MapGrids::load();
        assert_eq!(m.npc_corners.len(), 4, "по углу на каждый угол bbox");
        // все точки разные и каждая — в «своём» квадранте карты
        for (i, a) in m.npc_corners.iter().enumerate() {
            for b in m.npc_corners.iter().skip(i + 1) {
                assert!(a.distance(*b) > TILE, "углы не совпадают: {a:?} vs {b:?}");
            }
        }
        let sx = [-1.0, 1.0, -1.0, 1.0];
        let sy = [-1.0, -1.0, 1.0, 1.0];
        for (i, p) in m.npc_corners.iter().enumerate() {
            assert!(
                p.x * sx[i] > 0.0 && p.y * sy[i] > 0.0,
                "угол {i} не в своём квадранте: {p:?}"
            );
        }
        // угловые точки проходимы: не внутри стенового AABB
        for p in &m.npc_corners {
            for (min, max) in &m.wall_aabbs {
                let inside = p.x > min.x && p.x < max.x && p.y > min.y && p.y < max.y;
                assert!(!inside, "угол {p:?} внутри стены {min:?}..{max:?}");
            }
        }
    }
}
