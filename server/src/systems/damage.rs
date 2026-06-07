use bevy::prelude::*;
use bevy_quinnet::server::QuinnetServer;
use protocol::{
    combat::block_absorbs,
    constants::{BLOCK_ARC_HALF_ANGLE, CH_S2C},
    messages::S2C,
};

use protocol::constants::DAMAGE_REVEAL_TIME;

use crate::{
    events::DamageEvent,
    resources::{PlayerStates, RespawnDelay, RespawnQueue, RespawnTask, Reveals, SpawnPoints},
};

pub fn apply_damage(
    mut ev_damage: MessageReader<DamageEvent>,
    mut states: ResMut<PlayerStates>,
    mut respawn_q: ResMut<RespawnQueue>,
    delay: Res<RespawnDelay>,
    time: Res<Time>,
    mut server: ResMut<QuinnetServer>,
    mut reveals: ResMut<Reveals>,
    spawns: Res<SpawnPoints>,
) {
    let now = time.elapsed_secs_f64();
    for ev in ev_damage.read() {
        // «Подсветка» атакующего жертве на DAMAGE_REVEAL_TIME: даже если удар
        // прилетел сбоку/со спины (вне поля зрения), жертва увидит источник.
        if let Some(src) = ev.source {
            reveals
                .players
                .entry(ev.target)
                .or_default()
                .insert(src, now + DAMAGE_REVEAL_TIME);
        }
        if let Some(nsrc) = ev.npc_source {
            reveals
                .npcs
                .entry(ev.target)
                .or_default()
                .insert(nsrc, now + DAMAGE_REVEAL_TIME);
        }

        // позиция источника урона (для направленного блока): сперва явная (непись),
        // иначе из состояния игрока-источника. Берём до get_mut цели.
        let source_pos = ev
            .source_pos
            .or_else(|| ev.source.and_then(|sid| states.0.get(&sid).map(|s| s.pos)));

        if let Some(st) = states.0.get_mut(&ev.target) {
            // Активный блок ПОЛНОСТЬЮ гасит удар во фронтальной полусфере (±90° от
            // ПОВОРОТА МОДЕЛИ, st.rot — не от курсора). Сзади и без блока — проходит.
            let blocked = st.blocking
                && source_pos
                    .map(|sp| block_absorbs(st.pos, st.rot, sp, BLOCK_ARC_HALF_ANGLE))
                    .unwrap_or(false);
            let amount = if blocked { 0 } else { ev.amount };
            st.hp -= amount;

            // todo not work info! here
            println!(
                "🩸 Player {} took {} dmg (hp={}, blocking={})",
                ev.target, amount, st.hp, st.blocking
            );

            let endpoint = server.endpoint_mut();

            // send damage event
            endpoint
                .broadcast_message_on(
                    CH_S2C,
                    S2C::PlayerDamaged {
                        id: ev.target,
                        new_hp: st.hp,
                        damage: amount,
                    },
                )
                .ok();

            if st.hp <= 0 {
                // 1) сразу рассылаем PlayerDied
                if let Err(e) = endpoint.broadcast_message_on(
                    CH_S2C,
                    S2C::PlayerDied {
                        victim: ev.target,
                        killer: ev.source,
                    },
                ) {
                    warn!("broadcast PlayerDied failed: {e:?}");
                }
                info!("💀 [Server] Player {} died", ev.target);

                // 2) удаляем состояние и планируем респавн
                states.0.remove(&ev.target);

                // ОЧИЩАЕМ предыдущие задачи для этого игрока
                respawn_q.0.retain(|task| task.pid != ev.target);

                // ставим задачу на время now + delay (точка спавна — СЛУЧАЙНАЯ
                // из набора карты, чтобы не возрождаться всегда в одном месте)
                let seed = now.to_bits() ^ ev.target.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let spawn_pos = pick_spawn_point(&spawns, seed);
                respawn_q.0.push(RespawnTask {
                    pid: ev.target,
                    due: now + delay.0,
                    pos: spawn_pos,
                });

                info!(
                    "⏳ [Server] Scheduled respawn of {} at t={}",
                    ev.target,
                    now + delay.0
                );
            }
        }
    }

    // чистим истёкшие «подсветки», чтобы карты не росли бесконечно
    for m in reveals.players.values_mut() {
        m.retain(|_, &mut t| t > now);
    }
    reveals.players.retain(|_, m| !m.is_empty());
    for m in reveals.npcs.values_mut() {
        m.retain(|_, &mut t| t > now);
    }
    reveals.npcs.retain(|_, m| !m.is_empty());
}

/// Случайная точка спавна из набора карты (`SpawnPoints`). `seed` варьируется
/// от смерти к смерти (время ⊕ id), поэтому игрок появляется в разных местах.
fn pick_spawn_point(spawns: &SpawnPoints, seed: u64) -> Vec2 {
    if spawns.0.is_empty() {
        return Vec2::ZERO;
    }
    // дешёвый перемешиватель битов (SplitMix64-степень), без внешних крейтов
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let idx = (z % spawns.0.len() as u64) as usize;
    spawns.0[idx]
}
