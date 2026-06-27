//! Мини-лобби: список серверов из `client_config.toml`, каждый из которых
//! асинхронно проверяется на активность через лёгкий TCP-query (см.
//! `protocol::server_meta`). Здесь — только данные/опрос; UI живёт в `menu.rs`.

use bevy::prelude::*;
use protocol::server_meta::ServerMeta;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Таймаут на установку TCP-соединения и чтение меты.
const QUERY_TIMEOUT: Duration = Duration::from_millis(1500);

/// Состояние проверки одного сервера.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ServerStatus {
    /// Опрос ещё идёт.
    Checking,
    /// Сервер ответил: сколько игроков и лимит (0 — без лимита).
    Online { players: u32, max: u32 },
    /// Сервер не ответил/недоступен.
    Offline,
}

/// Один сервер в лобби (рантайм-состояние строки списка).
#[derive(Clone)]
pub struct LobbyServer {
    /// Имя для показа (из конфига; заменяется на имя с сервера, если пришло).
    pub name: String,
    /// Адрес `ip:port` (он же используется для подключения и для query).
    pub address: String,
    pub status: ServerStatus,
}

/// Текущий список серверов лобби. Строится при входе в меню из конфига.
#[derive(Resource, Default)]
pub struct LobbyServers(pub Vec<LobbyServer>);

/// Результат опроса одного сервера, который фоновый поток кладёт в инбокс.
pub struct QueryResult {
    pub address: String,
    pub status: ServerStatus,
    /// Имя, полученное с сервера (если ответил).
    pub name: Option<String>,
}

/// Разделяемый «почтовый ящик» результатов опроса. Потоки пишут, ECS-система
/// `drain_query_results` вычитывает. `Arc<Mutex<…>>` — чтобы быть Send+Sync для
/// хранения в ресурсе Bevy (в отличие от `mpsc::Receiver`).
#[derive(Resource, Clone, Default)]
pub struct QueryInbox(pub Arc<Mutex<Vec<QueryResult>>>);

/// Запускает фоновый опрос всех серверов из текущего `LobbyServers`.
/// Каждый сервер проверяется в своём коротком потоке; результат уходит в инбокс.
pub fn spawn_server_queries(servers: &LobbyServers, inbox: &QueryInbox) {
    for s in &servers.0 {
        let address = s.address.clone();
        let inbox = inbox.0.clone();
        let res = std::thread::Builder::new()
            .name("server-query".into())
            .spawn(move || {
                let (status, name) = match query_once(&address) {
                    Some(meta) => (
                        ServerStatus::Online {
                            players: meta.players,
                            max: meta.max_players,
                        },
                        Some(meta.name),
                    ),
                    None => (ServerStatus::Offline, None),
                };
                if let Ok(mut buf) = inbox.lock() {
                    buf.push(QueryResult {
                        address,
                        status,
                        name,
                    });
                }
            });
        if let Err(e) = res {
            warn!("не удалось запустить поток опроса сервера: {e}");
        }
    }
}

/// Один синхронный query: подключиться, прочитать мету, распарсить.
fn query_once(address: &str) -> Option<ServerMeta> {
    // DNS/парс адреса (поддерживает hostname:port и ip:port)
    let addr = address.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, QUERY_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
    let mut buf = String::new();
    // сервер пишет короткую мету и закрывает соединение → читаем до EOF
    stream.read_to_string(&mut buf).ok()?;
    ServerMeta::parse(&buf)
}

/// Переносит накопленные результаты опроса в `LobbyServers` (обновляет статус и,
/// если пришло, имя сервера). UI перерисуется отдельной системой по изменению.
pub fn drain_query_results(inbox: Res<QueryInbox>, mut servers: ResMut<LobbyServers>) {
    let drained: Vec<QueryResult> = match inbox.0.lock() {
        Ok(mut buf) if !buf.is_empty() => std::mem::take(&mut *buf),
        _ => return,
    };
    for r in drained {
        if let Some(s) = servers.0.iter_mut().find(|s| s.address == r.address) {
            s.status = r.status;
            if let Some(name) = r.name {
                if !name.is_empty() {
                    s.name = name;
                }
            }
        }
    }
}
