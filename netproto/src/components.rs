//! Реплицируемые компоненты протокола.
//!
//! Заменяют поля старых снапшотов (`PlayerSnapshot`/`NpcSnapshot`): теперь это
//! настоящие ECS-компоненты, которые Lightyear реплицирует с сервера клиентам.
//! Для локального игрока часть из них предсказывается (rollback), для остальных
//! сущностей — интерполируется.

use bevy::math::Curve;
use bevy::math::curve::{FunctionCurve, Interval};
use bevy::prelude::*;
use lightyear::prelude::PeerId;
use lightyear::prelude::Diffable;
use protocol::abilities::Abilities;
use protocol::messages::NpcKind;
use serde::{Deserialize, Serialize};

/// Маркер игрока + идентификатор управляющего им клиента (стабилен между
/// реконнектами благодаря `PeerId`).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Player(pub PeerId);

/// Имя аккаунта (для подписей/таблицы очков). Меняется редко.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerName(pub String);

/// Позиция в мире (мир. координаты). Full-предсказание для локального игрока,
/// линейная интерполяция для остальных.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Position(pub Vec2);

impl Diffable for Position {
    fn base_value() -> Self {
        Position(Vec2::ZERO)
    }

    fn diff(&self, new: &Self) -> Self {
        Position(new.0 - self.0)
    }

    fn apply_diff(&mut self, delta: &Self) {
        self.0 += delta.0;
    }
}

/// Презентационный угол модели (facing).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Rotation(pub f32);

impl Diffable for Rotation {
    fn base_value() -> Self {
        Rotation(0.0)
    }

    fn diff(&self, new: &Self) -> Self {
        Rotation(new.0 - self.0)
    }

    fn apply_diff(&mut self, delta: &Self) {
        self.0 += delta.0;
    }
}

/// Очки здоровья.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Health(pub i32);

/// Полное состояние способностей игрока (стамина, КД рывка/удара/щита, заряд
/// блока, стан, facing). Реплицируется и предсказывается у локального игрока — это
/// даёт детерминированный реконсил кулдаунов/стамины/стана через rollback (вместо
/// прежнего ручного «сидирования» из снапшота). Единый источник истины для этих
/// величин (стамина больше НЕ дублируется отдельным компонентом).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct AbilityState(pub Abilities);

/// Установлен ли сейчас блок (для визуала и резолва урона). Транзиентный вывод
/// `tick_abilities`, переустанавливается каждый тик.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Blocking(pub bool);

/// Маркер неписи + её тип (набор спрайтов). ИИ общий для всех типов.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Npc {
    pub kind: NpcKind,
}

/// Видимое состояние неписи (агро/идёт атака/остаток стана) для клиента.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct NpcRuntime {
    pub aggro: bool,
    pub attacking: bool,
    pub stun_left: f32,
}

// --- Ease для линейной интерполяции (используется `add_linear_interpolation`) ---

impl Ease for Position {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| {
            Position(Vec2::lerp(start.0, end.0, t))
        })
    }
}

impl Ease for Rotation {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| {
            Rotation(lerp_angle(start.0, end.0, t))
        })
    }
}

/// Интерполяция углов по кратчайшей дуге (иначе на переходе ±π рывок).
#[inline]
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    use core::f32::consts::{PI, TAU};
    let diff = (b - a + PI).rem_euclid(TAU) - PI;
    a + diff * t
}
