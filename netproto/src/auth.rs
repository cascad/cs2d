//! Авторизация + таблица очков поверх сообщений Lightyear.
//!
//! Поток: клиент после коннекта шлёт `Hello{name,password}` по надёжному
//! упорядоченному каналу `AuthChannel`. Сервер сверяет/регистрирует аккаунт
//! (`protocol::accounts::AccountBook`), при успехе отвечает `AuthOk` и спавнит
//! игрока (с этого момента он реплицируется/предсказывается), иначе `AuthDenied`.
//! Статистика живёт в аккаунте и переживает реконнект (тот же логин → та же
//! статистика, новый `PeerId` перепривязывается). Таблица очков рассылается всем
//! при изменениях (вход/выход/килл/смерть).

use bevy::prelude::*;
use lightyear::prelude::*;
use protocol::abilities::Abilities;
use protocol::accounts::{AccountBook, AuthOutcome};

use crate::map::MapGrids;
use crate::{
    AbilityState, AuthDenied, AuthOk, Blocking, Health, Hello, Player, PlayerName, Position,
    Rotation, Scoreboard,
};

/// Канал авторизации/служебных сообщений: надёжный и упорядоченный.
pub struct AuthChannel;

/// Реестр аккаунтов как ресурс (обёртка над чистым `AccountBook`).
#[derive(Resource, Default)]
pub struct Accounts(pub AccountBook);

/// Нужно ли разослать обновлённую таблицу очков в этом кадре.
#[derive(Resource, Default)]
pub struct ScoreboardDirty(pub bool);

/// Маркер на `ClientOf`: соединение уже авторизовано (игрок заспавнен).
#[derive(Component)]
pub struct Authed(pub PeerId);

/// Спавн авторизованного игрока: реплика всем, предсказание себе, интерполяция
/// прочим, привязка к соединению `link`, участие в тумане войны.
pub fn spawn_authed_player(
    commands: &mut Commands,
    peer: PeerId,
    link: Entity,
    name: String,
    spawn: Vec2,
) -> Entity {
    commands
        .spawn((
            Player(peer),
            PlayerName(name),
            Position(spawn),
            Rotation(0.0),
            Health(100),
            AbilityState(Abilities::default()),
            Blocking(false),
            Replicate::to_clients(NetworkTarget::All),
            PredictionTarget::to_clients(NetworkTarget::Single(peer)),
            InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer)),
            ControlledBy {
                owner: link,
                lifetime: Default::default(),
            },
            NetworkVisibility::default(),
        ))
        .id()
}

/// Серверная обработка `Hello`: авторизация + спавн игрока / отказ. Работает по
/// соединениям (`ClientOf`), ещё не помеченным `Authed`.
#[allow(clippy::type_complexity)]
pub fn server_handle_auth(
    mut commands: Commands,
    mut accounts: ResMut<Accounts>,
    mut dirty: ResMut<ScoreboardDirty>,
    map: Option<Res<MapGrids>>,
    mut spawn_idx: Local<usize>,
    mut links: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<Hello>,
            &mut MessageSender<AuthOk>,
            &mut MessageSender<AuthDenied>,
        ),
        Without<Authed>,
    >,
) {
    for (link, remote, mut rx, mut ok, mut denied) in &mut links {
        let Some(hello) = rx.receive().last() else {
            continue;
        };
        let peer = remote.0;
        let cid = peer.to_bits();
        match accounts.0.authenticate(cid, &hello.name, &hello.password) {
            AuthOutcome::Registered | AuthOutcome::Authenticated => {
                ok.send::<AuthChannel>(AuthOk);
                commands.entity(link).insert(Authed(peer));
                // Спавним игрока на реальной точке спавна карты (по кругу), а не в
                // (0,0): там может быть стена/НИП, из-за чего «бьют из ниоткуда».
                let spawn = map
                    .as_deref()
                    .map(|m| &m.spawns)
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        let p = s[*spawn_idx % s.len()];
                        *spawn_idx += 1;
                        p
                    })
                    .unwrap_or(Vec2::ZERO);
                let p = spawn_authed_player(&mut commands, peer, link, hello.name.clone(), spawn);
                dirty.0 = true;
                info!("AUTH ok: {} (peer={peer:?}) -> player {p:?}", hello.name);
            }
            AuthOutcome::WrongPassword => {
                denied.send::<AuthChannel>(AuthDenied {
                    reason: "wrong password".into(),
                });
                info!("AUTH denied (wrong password): {}", hello.name);
            }
            AuthOutcome::AlreadyOnline => {
                denied.send::<AuthChannel>(AuthDenied {
                    reason: "account already in game".into(),
                });
                info!("AUTH denied (already online): {}", hello.name);
            }
        }
    }
}

/// Рассылка таблицы очков всем клиентам, когда она изменилась.
pub fn broadcast_scoreboard(
    mut dirty: ResMut<ScoreboardDirty>,
    accounts: Res<Accounts>,
    mut senders: Query<&mut MessageSender<Scoreboard>>,
) {
    if !dirty.0 {
        return;
    }
    dirty.0 = false;
    let snap = accounts.0.snapshot();
    for mut s in &mut senders {
        s.send::<AuthChannel>(Scoreboard(snap.clone()));
    }
}

/// Очистка при дисконнекте: снять онлайн-привязку аккаунта (статистика остаётся)
/// и убрать игрока этого соединения; пометить таблицу к рассылке.
pub fn on_disconnect_cleanup(
    trigger: On<Add, Disconnected>,
    mut commands: Commands,
    mut accounts: ResMut<Accounts>,
    mut dirty: ResMut<ScoreboardDirty>,
    players: Query<(Entity, &Player, &ControlledBy)>,
) {
    let link = trigger.entity;
    for (e, player, cb) in &players {
        if cb.owner == link {
            accounts.0.unbind(player.0.to_bits());
            commands.entity(e).despawn();
            dirty.0 = true;
        }
    }
}
