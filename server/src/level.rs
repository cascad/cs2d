use bevy::prelude::{IVec2, Vec2};
use std::collections::HashSet;
use crate::resources::SolidTiles;

// todo rm
// pub fn load_fixed_level() -> (SolidTiles, Vec<Vec2>) {
//     let mut solid: HashSet<IVec2> = HashSet::new();

//     let w: i32 = 50;
//     let h: i32 = 50;

//     // внешний периметр
//     for x in 0..w {
//         solid.insert(IVec2::new(x, 0));
//         solid.insert(IVec2::new(x, h - 1));
//     }
//     for y in 0..h {
//         solid.insert(IVec2::new(0, y));
//         solid.insert(IVec2::new(w - 1, y));
//     }

//     // «стены-коридоры» внутри (две горизонтали и две вертикали)
//     for x in 0..w {
//         solid.insert(IVec2::new(x, 20));
//         solid.insert(IVec2::new(x, 30));
//     }
//     for y in 0..h {
//         solid.insert(IVec2::new(20, y));
//         solid.insert(IVec2::new(30, y));
//     }

//     // проёмы в этих стенах (чтобы были проходы)
//     solid.remove(&IVec2::new(25, 20));
//     solid.remove(&IVec2::new(25, 30));
//     solid.remove(&IVec2::new(20, 25));
//     solid.remove(&IVec2::new(30, 25));

//     // колонны 2x2 в угловых комнатах
//     for &(cx, cy) in &[(10, 10), (40, 10), (10, 40), (40, 40)] {
//         solid.insert(IVec2::new(cx, cy));
//         solid.insert(IVec2::new(cx + 1, cy));
//         solid.insert(IVec2::new(cx, cy + 1));
//         solid.insert(IVec2::new(cx + 1, cy + 1));
//     }

//     // точки спавна (центр тайла)
//     let to_center = |tx: i32, ty: i32| Vec2::new(tx as f32 + 0.5, ty as f32 + 0.5);
//     let spawns = vec![
//         to_center(5, 5),
//         to_center(45, 5),
//         to_center(5, 45),
//         to_center(45, 45),
//         to_center(25, 25),
//     ];

//     (SolidTiles(solid), spawns)
// }