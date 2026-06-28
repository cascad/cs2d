//! «Звёздочки» оглушения над головой игроков и неписей. Пока цель оглушена
//! (`stun_left > 0`), над ней горит значок из звёздочек.
//!
//! Реализовано как ОТДЕЛЬНЫЕ мировые `Text2d`-сущности, которые каждый кадр
//! следуют за целью — ровно тем же проверенным приёмом, что и всплывающий урон
//! (`spawn_damage_popups`). Раньше значок был дочерним `Text2d` под спрайтом, и
//! такой текст в нашей сцене не отрисовывался (особенно над неписями) — отсюда
//! «не видно, что моб в стане». Мировая сущность с высоким Z рисуется поверх всех
//! спрайтов и тумана, поэтому видна всегда.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::components::{ActorAnim, NpcMarker, StunStars};
use crate::systems::iso::ISO_ACTOR_PX;
use crate::systems::npc::NpcStun;

/// Высота значка над точкой ног игрока (экранные px) — выше макушки, полоски HP и
/// всплывающего урона, чтобы ни с чем не сливался.
const STARS_Y_PLAYER: f32 = ISO_ACTOR_PX * 1.18;
/// Высота значка над неписью — чуть выше её полоски HP (та на ~75px).
const STARS_Y_NPC: f32 = 104.0;
/// Z выше спрайтов/тумана/всплывающего урона — значок всегда сверху.
const STARS_Z: f32 = 960.0;
/// Текст значка: звёздочки-астериски. ВАЖНО: берём ASCII `*`, а не Unicode `★`
/// (U+2605) — в нашем шрифте FiraSans звезды нет, поэтому она рисовалась «тофу»-
/// прямоугольником. Астериск есть в любом шрифте и отрисовывается всегда.
const STARS_TEXT: &str = "* * *";
/// Цвет значка — яркий фиолетовый: читается и на светлом полу, и на тёмном.
const STARS_COLOR: Color = Color::srgb(0.78, 0.55, 1.0);

/// Поддерживает значки-звёздочки над оглушёнными: двигает существующие за целью,
/// гасит спавнит по состоянию стана (игроки — `ActorAnim.stun_left`, неписи —
/// ресурс `NpcStun`).
pub fn update_stun_stars(
    mut commands: Commands,
    asset: Res<AssetServer>,
    npc_stun: Res<NpcStun>,
    // только актёры (не сами значки): игрок (ActorAnim) или непись (NpcMarker)
    actors: Query<
        (Entity, &Transform, Option<&ActorAnim>, Option<&NpcMarker>),
        (Without<StunStars>, Or<(With<ActorAnim>, With<NpcMarker>)>),
    >,
    mut stars_q: Query<(Entity, &StunStars, &mut Transform, &mut Visibility)>,
) {
    // Цель → экранная позиция значка (над головой). Собираем только оглушённых.
    let mut stunned: HashMap<Entity, Vec3> = HashMap::new();
    for (e, tf, anim, npc) in actors.iter() {
        let (is_stunned, y) = if let Some(a) = anim {
            (a.stun_left > 0.0, STARS_Y_PLAYER)
        } else if let Some(m) = npc {
            (npc_stun.0.get(&m.0).copied().unwrap_or(0.0) > 0.0, STARS_Y_NPC)
        } else {
            (false, 0.0)
        };
        if is_stunned {
            stunned.insert(
                e,
                Vec3::new(tf.translation.x, tf.translation.y + y, STARS_Z),
            );
        }
    }

    // Существующие значки: тянем за целью, либо убираем (стан спал / цель исчезла).
    let mut have: HashSet<Entity> = HashSet::new();
    for (se, st, mut tf, mut vis) in stars_q.iter_mut() {
        match stunned.get(&st.target) {
            Some(pos) => {
                tf.translation = *pos;
                *vis = Visibility::Visible;
                have.insert(st.target);
            }
            None => commands.entity(se).despawn(),
        }
    }

    // Новым оглушённым (без значка) — спавним мировую Text2d-сущность.
    let font = asset.load("fonts/FiraSans-Bold.ttf");
    for (target, pos) in stunned {
        if have.contains(&target) {
            continue;
        }
        commands.spawn((
            Text2d::new(STARS_TEXT),
            TextFont {
                font: font.clone(),
                font_size: 32.0,
                ..default()
            },
            TextColor(STARS_COLOR),
            TextLayout::default(),
            Transform::from_translation(pos),
            GlobalTransform::default(),
            StunStars { target },
        ));
    }
}
