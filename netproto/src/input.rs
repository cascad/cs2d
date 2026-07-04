//! Пользовательский ввод, который клиент шлёт серверу (tick-synced).
//!
//! Заменяет `protocol::messages::InputState`. Lightyear хранит его в компоненте
//! `ActionState<NetInput>`; клиент пишет ввод в `FixedPreUpdate`, сервер читает в
//! `FixedUpdate`. Поля соответствуют намерениям из `AbilityInput` + бросок банки.

use bevy::ecs::entity::{EntityMapper, MapEntities};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Намерения игрока на один тик. `aim` — желаемый угол по курсору; модель
/// доворачивается к нему детерминированно в `tick_abilities`. `throw` — точка
/// броска банки (если в этот тик отпущена G), сервер клампит до дальности.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, Reflect)]
pub struct NetInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub aim: f32,
    pub block: bool,
    pub dash: bool,
    pub attack: bool,
    pub stun: bool,
    /// Цель броска банки на этом тике (мир. координаты), если бросок запрошен.
    pub throw: Option<Vec2>,
}

impl MapEntities for NetInput {
    fn map_entities<M: EntityMapper>(&mut self, _entity_mapper: &mut M) {}
}

impl NetInput {
    /// Вектор движения из нажатых клавиш (не нормализован).
    pub fn move_dir(&self) -> Vec2 {
        let mut d = Vec2::ZERO;
        if self.up {
            d.y += 1.0;
        }
        if self.down {
            d.y -= 1.0;
        }
        if self.right {
            d.x += 1.0;
        }
        if self.left {
            d.x -= 1.0;
        }
        d
    }
}
