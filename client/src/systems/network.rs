use std::str::FromStr;

use crate::app_state::AppState;
use crate::components::{Corpse, GrenadeNet, LocalPlayer, PlayerMarker};
use crate::constants::{BULLET_SPEED, BULLET_TTL};
use crate::render::{layers, world_to_translation, RenderLayer, WorldPos};
use crate::events::{
    GrenadeDetonatedEvent, GrenadeSpawnEvent, PlayerDamagedEvent, PlayerDied, PlayerLeftEvent,
};
use crate::menu::ConnectTimeout;
use crate::resources::grenades::{GrenadeStates, NetState};
use crate::resources::{
    ClientLatency, DeadPlayers, HpUiMap, LastKnownPos, LastSeen, LocalAbilities, LocalStatus,
    MyPlayer, PendingInputsClient, PredictedPos, RingTex, SnapshotBuffer, SpawnedPlayers, TimeSync,
    WallGridRes,
};
use crate::systems::iso::{ISO_ACTOR_PX, KNIGHT_ANCHOR_Y};
use crate::systems::shoot::spawn_tracer;
use crate::systems::utils::{spawn_hp_ui, time_in_seconds};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_quinnet::client::QuinnetClient;
use protocol::abilities::{tick_abilities, AbilityConfig, AbilityInput, Abilities};
use protocol::constants::{CH_S2C, PLAYER_SIZE, TICK_DT};
use protocol::geom::WallGrid;
use protocol::messages::{InputState, S2C};

#[derive(SystemParam)]
pub struct NetCtx<'w, 's> {
    pub commands: Commands<'w, 's>,

    pub buffer: ResMut<'w, SnapshotBuffer>,
    pub time_sync: ResMut<'w, TimeSync>,
    pub my: ResMut<'w, MyPlayer>,
    pub pending: ResMut<'w, PendingInputsClient>,
    pub spawned: ResMut<'w, SpawnedPlayers>,
    pub dead: ResMut<'w, DeadPlayers>,
    pub latency: ResMut<'w, ClientLatency>,
    pub hp_ui_map: ResMut<'w, HpUiMap>,

    pub q_marker: Query<'w, 's, (Entity, &'static PlayerMarker)>,

    pub ring_tex: Res<'w, RingTex>,
    pub knight: Res<'w, crate::systems::iso::KnightAnims>,

    // события
    pub ev_damage: MessageWriter<'w, PlayerDamagedEvent>,
    pub ev_died: MessageWriter<'w, PlayerDied>,
    pub ev_left: MessageWriter<'w, PlayerLeftEvent>,
    pub ev_grenade_spawn: MessageWriter<'w, GrenadeSpawnEvent>,
    pub ev_grenade_detonated: MessageWriter<'w, GrenadeDetonatedEvent>,

    // прочее
    pub grenade_states: ResMut<'w, GrenadeStates>,
    pub wall_grid: Res<'w, WallGridRes>,
    pub last_pos: Option<ResMut<'w, LastKnownPos>>,
    pub last_seen: ResMut<'w, LastSeen>,
    pub app_state: Res<'w, State<AppState>>,
    pub next_state: ResMut<'w, NextState<AppState>>,
    pub status: ResMut<'w, LocalStatus>,
    pub predicted: ResMut<'w, PredictedPos>,
    /// Живое предсказание способностей локального игрока — реконсилим в нём КД удара.
    pub abilities: ResMut<'w, LocalAbilities>,
    /// Анимации актёров по marker — чтобы запускать удар у удалённых игроков.
    pub q_anim: Query<'w, 's, (&'static PlayerMarker, &'static mut crate::components::ActorAnim)>,

    // НЕПИСИ (скелеты/зомби)
    pub skeleton: Option<Res<'w, crate::systems::npc::NpcAnims>>,
    pub spawned_npcs: ResMut<'w, crate::systems::npc::SpawnedNpcs>,
    pub q_npc: Query<'w, 's, (Entity, &'static crate::components::NpcMarker)>,

    /// Общий реестр трупов (скелеты + игроки) с лимитом MAX_CORPSES.
    pub corpses: ResMut<'w, crate::resources::Corpses>,

    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<ColorMaterial>>,
}

pub fn receive_server_messages(mut client: ResMut<QuinnetClient>, mut net: NetCtx) {
    // если дефолтного соединения нет — выходим
    let Some(conn) = client.get_connection_mut() else {
        return;
    };

    while let Some(msg) = conn.try_receive_message_on::<S2C, _>(CH_S2C) {
        match msg {
            // ===================================================
            // 1) СНАПШОТ
            // ===================================================
            S2C::Snapshot(snap) => {
                let now_client = time_in_seconds();

                // time sync один раз на пустом буфере
                if net.buffer.snapshots.is_empty() {
                    net.time_sync.offset = now_client - snap.server_time;
                    info!("[Network] time sync offset = {:.3}", net.time_sync.offset);
                }

                // reconciliation локального игрока: правим ТОЧНУЮ предсказанную
                // позицию (PredictedPos), а не Transform — отрисовку плавно
                // подтянет smooth_local_player.
                if let Some(ack) = snap.last_input_seq.get(&net.my.id) {
                    if let Some(ps) = snap.players.iter().find(|p| p.id == net.my.id) {
                        // стамина/блок/HP локального игрока — авторитет сервера (для UI)
                        net.status.stamina = ps.stamina;
                        net.status.blocking = ps.blocking;
                        net.status.hp = ps.hp;

                        while let Some(front) = net.pending.0.front() {
                            if front.seq <= *ack {
                                net.pending.0.pop_front();
                            } else {
                                break;
                            }
                        }
                        // переигрываем неподтверждённые вводы поверх авторитетной
                        // позиции тем же кодом, что и предсказание/сервер. КД удара
                        // СИДИРУЕМ авторитетным значением из снапшота — иначе
                        // клиентский счётчик жил бы сам по себе и расходился с
                        // сервером (тогда предсказанный взмах мог «не нанести урон»).
                        let cfg = AbilityConfig::default();
                        let mut ab = Abilities {
                            stamina: ps.stamina,
                            facing: ps.rotation,
                            melee_cd_left: ps.melee_cd_left,
                            ..Default::default()
                        };
                        let mut pos = Vec2::new(ps.x, ps.y);
                        for inp in net.pending.0.iter() {
                            pos = step_pos(pos, inp, &mut ab, &cfg, &net.wall_grid.0);
                        }
                        // Авторитетная позиция после переигрывания вводов. Отрисовка
                        // (smooth_local_player) мягко стянется к ней по dead-reckoning,
                        // не теряя ровной скорости — коррекция незаметна.
                        net.predicted.pos = pos;
                        net.predicted.valid = true;

                        // Реконсилим КД удара (и стамину/заморозку) в ЖИВОЕ
                        // предсказание: дальше send_input продолжает счёт ровно с
                        // авторитетного значения → клиент и сервер не расходятся.
                        // Рывок НЕ трогаем (он предсказывается клиентом и приходит
                        // событием DashFx — снапшот его КД не несёт).
                        net.abilities.0.melee_cd_left = ab.melee_cd_left;
                        net.abilities.0.attack_lock_left = ab.attack_lock_left;
                        net.abilities.0.stamina = ab.stamina;
                    }
                }

                // спавним новых из снапшота, обновляем HP-UI и last_pos
                for p in &snap.players {
                    let id = p.id;

                    if net.dead.0.contains(&id) {
                        continue;
                    }

                    // отмечаем «видели только что» — для тумана войны (fade/деспавн)
                    net.last_seen.0.insert(id, now_client);

                    if net.spawned.0.insert(p.id) {
                        let label = String::from_str("snapshot").unwrap();
                        spawn_player(
                            &mut net.commands,
                            &net.my,
                            id,
                            p.x,
                            p.y,
                            p.rotation,
                            label,
                            net.knight.idle.clone(),
                            net.knight.layout.clone(),
                            net.ring_tex.0.clone(),
                        );
                    }

                    if !net.hp_ui_map.0.contains_key(&p.id) {
                        let entity = spawn_hp_ui(&mut net.commands, p.id, p.hp);
                        net.hp_ui_map.0.insert(p.id, entity);
                    }

                    if let Some(last_pos) = net.last_pos.as_deref_mut() {
                        last_pos.0.insert(p.id, (Vec2::new(p.x, p.y), p.rotation));
                    }
                }

                // Поза блока УДАЛЁННЫХ игроков — из авторитетного снапшота
                // (`blocking`), иначе её видел только сам блокирующий. Локального
                // не трогаем: ему блок ставит предсказание (send_input).
                for p in &snap.players {
                    if p.id == net.my.id || net.dead.0.contains(&p.id) {
                        continue;
                    }
                    for (marker, mut anim) in net.q_anim.iter_mut() {
                        if marker.0 == p.id {
                            anim.blocking = p.blocking;
                            break;
                        }
                    }
                }

                // переход из Connecting в InGame по первому снапу
                if matches!(net.app_state.get(), AppState::Connecting) {
                    net.commands.remove_resource::<ConnectTimeout>();
                    net.next_state.set(AppState::InGame);
                }

                // буфер для интерполяции
                net.buffer.snapshots.push_back(snap);
                while net.buffer.snapshots.len() > 120 {
                    net.buffer.snapshots.pop_front();
                }
            }

            // ===================================================
            // 2) СТРЕЛЬБА
            // ===================================================
            S2C::ShootFx(fx) => {
                info!("💥 [Client] got FX from {} at {:?}", fx.shooter_id, fx.from);

                let max_dist = BULLET_SPEED * BULLET_TTL;
                let dir = fx.dir.normalize_or_zero();
                let hit_dist = net.wall_grid.0.raycast(fx.from, dir, max_dist);

                if hit_dist > 0.5 {
                    let ttl = hit_dist / BULLET_SPEED;
                    spawn_tracer(&mut net.commands, fx.from, dir, ttl);
                }
            }

            // ===================================================
            // 3) СПАВН / РЕСПАВН
            // ===================================================
            S2C::PlayerConnected { id, x, y } | S2C::PlayerRespawn { id, x, y } => {
                net.dead.0.remove(&id);
                net.buffer.snapshots.clear();

                if net.spawned.0.remove(&id) {
                    for (ent, marker) in net.q_marker.iter() {
                        if marker.0 == id {
                            net.commands.entity(ent).despawn();
                            break;
                        }
                    }
                }

                let rotation = 0.0;
                let label = String::from_str("new/respawn").unwrap();
                spawn_player(
                    &mut net.commands,
                    &net.my,
                    id,
                    x,
                    y,
                    rotation,
                    label,
                    net.knight.idle.clone(),
                    net.knight.layout.clone(),
                    net.ring_tex.0.clone(),
                );
                net.spawned.0.insert(id);

                // для локального игрока сбрасываем предсказание на точку спавна,
                // чтобы рендер не «уезжал» сглаживанием от старой позиции
                if id == net.my.id {
                    net.predicted.pos = Vec2::new(x, y);
                    net.predicted.vel = Vec2::ZERO;
                    net.predicted.valid = true;
                    net.pending.0.clear();
                }

                if let Some(last_pos) = net.last_pos.as_deref_mut() {
                    last_pos.0.insert(id, (Vec2::new(x, y), rotation));
                }
            }

            // ===================================================
            // 4) ИГРОК ВЫШЕЛ
            // ===================================================
            S2C::PlayerLeft(left_id) => {
                net.dead.0.remove(&left_id);

                if let Some((entity, _)) =
                    net.q_marker.iter().find(|(_, marker)| marker.0 == left_id)
                {
                    net.commands.entity(entity).despawn();
                    net.spawned.0.remove(&left_id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", left_id);
                }

                net.ev_left.write(PlayerLeftEvent(left_id));
            }

            // дубль, если прилетел другой тип уведомления
            S2C::PlayerDisconnected { id } => {
                net.dead.0.remove(&id);

                if let Some((entity, _)) = net.q_marker.iter().find(|(_, marker)| marker.0 == id) {
                    net.commands.entity(entity).despawn();
                    net.spawned.0.remove(&id);
                    info!("🔌 PlayerLeft: игрок {} вышел — despawn", id);
                }

                net.ev_left.write(PlayerLeftEvent(id));
            }

            // ===================================================
            // 5) PONG
            // ===================================================
            S2C::Pong {
                client_time,
                server_time,
            } => {
                let now = time_in_seconds();
                let rtt = now - client_time;
                let one_way = (rtt - (now - server_time)) * 0.5;
                net.latency.rtt = rtt;
                net.latency.offset = server_time - (client_time + one_way);
            }

            // ===================================================
            // 6) ДАМАГ
            // ===================================================
            S2C::PlayerDamaged { id, new_hp, damage } => {
                // HUD локального игрока обновляем СРАЗУ из события урона: смертельный
                // удар убирает игрока из снапшотов (его hp<=0 не приходит в снапшоте),
                // поэтому без этого полоска HUD не успевала показать обнуление.
                if id == net.my.id {
                    net.status.hp = new_hp;
                }
                net.ev_damage
                    .write(PlayerDamagedEvent { id, new_hp, damage });
            }

            // ===================================================
            // 7) ГРАНАТЫ
            // ===================================================
            S2C::GrenadeSpawn(ev) => {
                let printable_ev = ev.clone();
                net.ev_grenade_spawn.write(GrenadeSpawnEvent(ev));
                info!("💣 GrenadeSpawn {}", printable_ev.id);
            }

            S2C::GrenadeSync { id, pos, vel, ts } => {
                // info!("grenades sync: {:?}", pos);
                let e = net.grenade_states.0.entry(id).or_default();
                *e = NetState {
                    pos,
                    vel,
                    ts,
                    has: true,
                };
            }

            S2C::GrenadeDetonated { id, pos } => {
                net.ev_grenade_detonated
                    .write(GrenadeDetonatedEvent { id, pos });
            }

            // ===================================================
            // 7.5) MELEE-ВЗМАХ (визуал)
            // ===================================================
            S2C::MeleeFx {
                attacker_id,
                from,
                dir,
            } => {
                if attacker_id != net.my.id {
                    for (marker, mut anim) in net.q_anim.iter_mut() {
                        if marker.0 == attacker_id {
                            anim.start_action(crate::components::AnimState::Attack);
                        }
                    }
                    crate::systems::melee::spawn_player_melee_decal(
                        &mut net.commands,
                        &mut net.meshes,
                        &mut net.materials,
                        from,
                        dir,
                    );
                }
            }

            S2C::DashFx { player_id, dir } => {
                // Источник правды о рывке — сервер: ролл играем для ЛЮБОГО игрока
                // (вкл. локального) ровно на реальный рывок, спрайт катится в `dir`.
                let ang = if dir.length_squared() > 1e-6 {
                    dir.y.atan2(dir.x)
                } else {
                    0.0
                };
                for (marker, mut anim) in net.q_anim.iter_mut() {
                    if marker.0 == player_id {
                        anim.start_action_facing(crate::components::AnimState::Dash, ang);
                    }
                }
            }

            // ===================================================
            // 8) СМЕРТЬ
            // ===================================================
            S2C::PlayerDied { victim, killer } => {
                info!("[Client]   PlayerDied victim={}", victim);

                if let Some(last_pos) = net.last_pos.as_ref() {
                    if let Some((pos, rot)) = last_pos.0.get(&victim).cloned() {
                        use crate::systems::iso::{knight_row, KNIGHT_COLS};
                        let row = knight_row(rot);
                        // Труп — анимация смерти рыцаря (Death.png), проигрывается
                        // один раз и замирает на последнем кадре; ряд = направление.
                        let corpse_ent = net.commands.spawn((
                            Sprite {
                                image: net.knight.death.clone(),
                                texture_atlas: Some(TextureAtlas {
                                    layout: net.knight.layout.clone(),
                                    index: row * KNIGHT_COLS,
                                }),
                                custom_size: Some(Vec2::splat(ISO_ACTOR_PX)),
                                ..default()
                            },
                            bevy::sprite::Anchor(Vec2::new(0.0, KNIGHT_ANCHOR_Y)),
                            Transform::from_translation(world_to_translation(pos, layers::CORPSE)),
                            GlobalTransform::default(),
                            WorldPos(pos),
                            RenderLayer(layers::CORPSE),
                            Corpse {
                                row,
                                frame: 0,
                                anim: Timer::from_seconds(1.0 / 24.0, TimerMode::Repeating),
                            },
                        )).id();
                        net.corpses.register(&mut net.commands, corpse_ent);
                    }
                }

                net.dead.0.insert(victim);

                if victim == net.my.id {
                    for (ent, _) in net.q_marker.iter().filter(|(_, m)| m.0 == victim) {
                        net.commands.entity(ent).despawn();
                        net.spawned.0.remove(&victim);
                    }
                    net.buffer.snapshots.clear();
                } else if let Some((ent, _)) = net.q_marker.iter().find(|(_, m)| m.0 == victim) {
                    net.commands.entity(ent).despawn();
                    net.spawned.0.remove(&victim);
                }

                net.ev_died.write(PlayerDied { victim, killer });
                info!("💀 Игрок {} погиб ({:?})", victim, killer);
            }

            // ===================================================
            // 9) СМЕРТЬ НЕПИСЯ (скелета) → труп
            // ===================================================
            S2C::NpcDied { id, x, y, facing, kind } => {
                if let Some((ent, _)) = net.q_npc.iter().find(|(_, m)| m.0 == id) {
                    net.commands.entity(ent).despawn();
                }
                net.spawned_npcs.0.remove(&id);
                if let Some(anims) = net.skeleton.as_deref() {
                    let corpse_ent = crate::systems::npc::spawn_npc_corpse(
                        &mut net.commands,
                        anims,
                        kind,
                        Vec2::new(x, y),
                        facing,
                    );
                    net.corpses.register(&mut net.commands, corpse_ent);
                }
            }
        }
    }
}

/// Один шаг реплея ввода поверх позиции (тем же кодом, что предсказание и
/// сервер). Возвращает новую позицию.
fn step_pos(
    pos: Vec2,
    inp: &InputState,
    ab: &mut Abilities,
    cfg: &AbilityConfig,
    walls: &WallGrid,
) -> Vec2 {
    let mut dir = Vec2::ZERO;
    if inp.up {
        dir.y += 1.0;
    }
    if inp.down {
        dir.y -= 1.0;
    }
    if inp.left {
        dir.x -= 1.0;
    }
    if inp.right {
        dir.x += 1.0;
    }
    dir = dir.normalize_or_zero();
    let out = tick_abilities(
        ab,
        &AbilityInput {
            move_dir: dir,
            aim: inp.rotation,
            want_dash: inp.dash,
            want_block: inp.block,
            want_attack: inp.attack,
        },
        TICK_DT,
        cfg,
    );
    let delta = out.move_dir * out.speed * TICK_DT;
    walls.slide_circle(pos, delta, PLAYER_SIZE * 0.5)
}

/// Локальная (в системе координат спрайта игрока = экранной) позиция засечки на
/// ободке-эллипсе для мирового угла `facing`. Точка наземного круга радиуса `RW`
/// под углом `facing` проецируется в изо — так засечка ложится ровно на обод.
fn notch_local(facing: f32) -> Vec3 {
    use crate::render::world_to_screen;
    // полуось эллипса по X = ISO_ACTOR_PX*0.21 (= RW*√2) ⇒ RW = .../√2
    let rw = ISO_ACTOR_PX * 0.21 / std::f32::consts::SQRT_2;
    let s = world_to_screen(Vec2::new(facing.cos(), facing.sin())) * rw;
    Vec3::new(s.x, s.y, -0.45) // чуть выше кольца (-0.5), но под телом
}

/// Каждый кадр двигает засечку по ободку в направлении взгляда игрока (`Facing`).
pub fn update_dir_notch(
    q_face: Query<&crate::components::Facing>,
    mut q_notch: Query<(&ChildOf, &mut Transform), With<crate::components::DirNotch>>,
) {
    for (parent, mut tf) in q_notch.iter_mut() {
        if let Ok(facing) = q_face.get(parent.parent()) {
            tf.translation = notch_local(facing.0);
        }
    }
}

// Утилита для единообразного создания сущности игрока.
// Тело — круг (совпадает с круговой коллизией), плюс дочерний «ствол» —
// прямоугольник, торчащий вперёд по направлению взгляда (локальная ось +X).
fn spawn_player(
    commands: &mut Commands,
    me: &ResMut<MyPlayer>,
    id: u64,
    x: f32,
    y: f32,
    rot: f32,
    from: String,
    sheet: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    ring: Handle<Image>,
) -> Entity {
    use crate::systems::iso::{knight_row, KNIGHT_COLS};
    let world = Vec2::new(x, y);
    // Направленный спрайт НЕ вращаем — направление кодируется выбором кадра/ряда.
    let tf = Transform::from_translation(world_to_translation(world, layers::ACTOR));
    let is_local = id == me.id;

    // HD-рыцарь и так светлый — не тонируем тело (свой/чужой различает кольцо).
    let entity = commands
        .spawn((
            Sprite {
                image: sheet,
                texture_atlas: Some(TextureAtlas {
                    layout,
                    index: knight_row(rot) * KNIGHT_COLS,
                }),
                custom_size: Some(Vec2::splat(ISO_ACTOR_PX)),
                ..default()
            },
            // Якорим «ноги» рыцаря к WorldPos, чтобы кольцо-маркер было ровно под
            // ногами. В Bevy 0.18 Anchor — отдельный компонент рядом со Sprite.
            bevy::sprite::Anchor(Vec2::new(0.0, KNIGHT_ANCHOR_Y)),
            tf,
            GlobalTransform::default(),
            WorldPos(world),
            RenderLayer(layers::ACTOR),
            PlayerMarker(id),
            crate::components::Facing(rot),
            crate::components::ActorAnim {
                prev: world,
                ..default()
            },
            Name::new(format!(
                "Player[{}] {}",
                if is_local { "LOCAL" } else { "REMOTE" },
                id
            )),
        ))
        .with_children(|p| {
            // Диабло-кольцо под ногами (полый ободок, изо-эллипс): всегда находимый
            // маркер, заодно различает свой/чужой. Центр = WorldPos (ноги).
            let ring_color = if is_local {
                Color::srgba(0.3, 1.0, 0.5, 0.95)
            } else {
                Color::srgba(1.0, 0.35, 0.35, 0.9)
            };
            p.spawn((
                Sprite {
                    image: ring,
                    color: ring_color,
                    // изо-эллипс (2:1 сплющен) под ногами
                    custom_size: Some(Vec2::new(ISO_ACTOR_PX * 0.42, ISO_ACTOR_PX * 0.21)),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, -0.5),
            ));
            // Засечка направления на ободке (куда смотрит/целится модель).
            p.spawn((
                Sprite {
                    color: Color::srgb(1.0, 1.0, 1.0),
                    custom_size: Some(Vec2::splat(ISO_ACTOR_PX * 0.10)),
                    ..default()
                },
                Transform::from_translation(notch_local(rot)),
                crate::components::DirNotch,
            ));
        })
        .id();

    if is_local {
        // Маркер, что это локальный игрок
        commands.entity(entity).insert(LocalPlayer);
        info!("[Client]{from} spawn LOCAL {}", id);
    } else {
        info!("[Client][{from}] spawn REMOTE {}", id);
    }

    entity
}

/// Применяем сетевое состояние к Transform гранат.
/// Между снапшотами — лёгкая экстраполяция pos += vel * (now - ts)
pub fn apply_grenade_net(
    states: Res<GrenadeStates>,
    time_sync: Res<TimeSync>,
    mut q: Query<(&GrenadeNet, &mut WorldPos)>,
) {
    let now_server = time_in_seconds() - time_sync.offset; // серверные секунды
    for (net, mut wp) in q.iter_mut() {
        if let Some(s) = states.0.get(&net.id) {
            if !s.has {
                continue;
            }
            let mut dt = (now_server - s.ts) as f32;
            if !time_sync.offset.is_finite() {
                dt = 0.0;
            }
            dt = dt.clamp(0.0, 0.25);
            wp.0 = s.pos + s.vel * dt;
        }
    }
}
