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

/// Лимиты сервера (вставляет `server/main.rs` из конфига). `max_players == 0`
/// — без лимита (не рекомендуется для публичного сервера).
#[derive(Resource, Default)]
pub struct ServerLimits {
    pub max_players: u32,
}

/// Путь к файлу персистентности аккаунтов (вставляет `server/main.rs`).
/// Без ресурса аккаунты живут только в памяти (как раньше).
#[derive(Resource, Clone)]
pub struct AccountsFile(pub std::path::PathBuf);

/// Загружает аккаунты с диска (или пустой реестр, если файла нет/битый).
pub fn load_accounts(path: &std::path::Path) -> Accounts {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<AccountBook>(&text) {
            Ok(book) => {
                info!("💾 аккаунты загружены: {} ({} шт.)", path.display(), book.snapshot().len());
                Accounts(book)
            }
            Err(e) => {
                warn!("💾 битый файл аккаунтов {} ({e}) — стартуем с пустого", path.display());
                Accounts::default()
            }
        },
        Err(_) => {
            info!("💾 файла аккаунтов нет ({}) — первый запуск", path.display());
            Accounts::default()
        }
    }
}

/// Атомарно сохраняет аккаунты (tmp → rename): обрыв записи не портит файл.
fn save_accounts(path: &std::path::Path, book: &AccountBook) {
    let Ok(json) = serde_json::to_string_pretty(book) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("💾 не удалось сохранить аккаунты в {}: {e}", path.display());
    }
}

/// Максимальная длина имени игрока (обрезается сервером; защита от «имён»
/// на мегабайт в таблице очков и трафике репликации).
const MAX_NAME_LEN: usize = 24;

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
    limits: Option<Res<ServerLimits>>,
    players: Query<(), With<Player>>,
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
    let max_players = limits.as_deref().map(|l| l.max_players).unwrap_or(0);
    // Счётчик снаружи цикла + инкремент на каждый спавн: несколько Hello в ОДИН
    // кадр не должны пролезать под лимит одновременно.
    let mut online = players.iter().count() as u32;
    for (link, remote, mut rx, mut ok, mut denied) in &mut links {
        let Some(hello) = rx.receive().last() else {
            continue;
        };
        // Лимит игроков: без него любой желающий кладёт сервер толпой клиентов.
        if max_players > 0 && online >= max_players {
            denied.send::<AuthChannel>(AuthDenied {
                reason: format!("сервер полон ({online}/{max_players}), попробуйте позже"),
            });
            info!("AUTH denied (server full {online}/{max_players}): {}", hello.name);
            continue;
        }
        // Санитизация имени: обрезка пробелов и длины (защита от мусорных имён).
        let name: String = hello.name.trim().chars().take(MAX_NAME_LEN).collect();
        if name.is_empty() {
            denied.send::<AuthChannel>(AuthDenied {
                reason: "пустое имя".into(),
            });
            continue;
        }
        let peer = remote.0;
        let cid = peer.to_bits();
        match accounts.0.authenticate(cid, &name, &hello.password) {
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
                let p = spawn_authed_player(&mut commands, peer, link, name.clone(), spawn);
                dirty.0 = true;
                online += 1;
                info!("AUTH ok: {name} (peer={peer:?}) -> player {p:?} [{online}/{max_players}]");
            }
            AuthOutcome::WrongPassword => {
                denied.send::<AuthChannel>(AuthDenied {
                    reason: "неверный пароль".into(),
                });
                info!("AUTH denied (wrong password): {name}");
            }
            AuthOutcome::AlreadyOnline => {
                denied.send::<AuthChannel>(AuthDenied {
                    reason: "аккаунт уже в игре".into(),
                });
                info!("AUTH denied (already online): {name}");
            }
        }
    }
}

/// Рассылка таблицы очков всем клиентам, когда она изменилась. Заодно — точка
/// персистентности: `ScoreboardDirty` взводится ровно на тех событиях, которые
/// меняют аккаунты (вход/килл/смерть), так что здесь же сохраняем их на диск.
pub fn broadcast_scoreboard(
    mut dirty: ResMut<ScoreboardDirty>,
    accounts: Res<Accounts>,
    file: Option<Res<AccountsFile>>,
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
    if let Some(f) = file {
        save_accounts(&f.0, &accounts.0);
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
