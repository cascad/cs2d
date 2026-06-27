use bevy::prelude::Resource;
use bevy_quinnet::shared::channels::{ChannelConfig, SendChannelsConfiguration};
use protocol::channels::{CHANNELS, Reliability};
use protocol::server_meta::ServerMeta;
use std::io::Write;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Собираем `SendChannelsConfiguration` из описания протокола
pub fn channels_config() -> SendChannelsConfiguration {
    let kinds = CHANNELS.iter().map(|desc| match desc.reliability {
        Reliability::OrderedReliable { max_frame_size } =>
            ChannelConfig::OrderedReliable { max_frame_size },
        Reliability::UnorderedReliable { max_frame_size } =>
            ChannelConfig::UnorderedReliable { max_frame_size },
    }).collect::<Vec<_>>();

    SendChannelsConfiguration::from_configs(kinds).expect("invalid channel config")
}

/// Разделяемый счётчик игроков для меты лобби. Игровой цикл обновляет его, а
/// фоновый TCP-листенер читает при ответе на query-запросы.
#[derive(Resource, Clone)]
pub struct MetaPlayerCount(pub Arc<AtomicU32>);

/// Поднимает фоновый TCP-листенер меты на `ip:port` (тот же номер порта, что и
/// игровой QUIC). На каждое входящее соединение отдаёт `ServerMeta` и закрывает
/// его. Возвращает разделяемый счётчик игроков, который должен обновлять
/// игровой цикл. При ошибке привязки сокета мета просто не работает (лобби
/// покажет сервер как офлайн), но игра продолжает запускаться.
pub fn start_meta_endpoint(ip: IpAddr, port: u16, name: String, max_players: u32) -> MetaPlayerCount {
    let players = Arc::new(AtomicU32::new(0));
    let players_for_thread = players.clone();
    let addr = SocketAddr::new(ip, port);

    std::thread::Builder::new()
        .name("meta-endpoint".into())
        .spawn(move || {
            let listener = match TcpListener::bind(addr) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("⚠ мета-листенер не поднялся на {addr} (TCP): {e}");
                    return;
                }
            };
            println!("📡 Meta endpoint (lobby) on {addr} (TCP)");
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => handle_meta_conn(s, &name, max_players, &players_for_thread),
                    Err(e) => eprintln!("⚠ мета: ошибка accept: {e}"),
                }
            }
        })
        .expect("failed to spawn meta-endpoint thread");

    MetaPlayerCount(players)
}

/// Отвечает одному query-клиенту: пишет мету и закрывает соединение. Таймаут на
/// запись защищает от подвисших клиентов.
fn handle_meta_conn(mut stream: TcpStream, name: &str, max_players: u32, players: &Arc<AtomicU32>) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let meta = ServerMeta {
        name: name.to_string(),
        players: players.load(Ordering::Relaxed),
        max_players,
    };
    let _ = stream.write_all(meta.encode().as_bytes());
    let _ = stream.flush();
}