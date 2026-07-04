# Сборка headless-сервера CS2D. Контекст сборки — КОРЕНЬ репозитория
# (нужны vendor/ с патчами lightyear и Cargo.lock), см. docker-compose.yml.
# 1.93+ — MSRV зависимостей (fixed 1.31 требует 1.93, bevy 0.18 — 1.89)
FROM rust:1.93-bookworm AS build
WORKDIR /src
COPY . .
# Только сервер: клиент/wasm собираются отдельно (trunk build на машине разработчика).
RUN cargo build --release -p server

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /src/target/release/server /app/server
# server_config.toml создаётся рядом с бинарём при первом старте (дефолты:
# 0.0.0.0:6000 + WebTransport :6001); свой можно смонтировать поверх.
# digest самоподписанного сертификата пишется в /app/certificates/digest.txt —
# каталог шарится с веб-сервером (см. docker-compose.yml).
EXPOSE 6000/udp 6000/tcp 6001/udp
# TCP :6000 — мета лобби: отвечает только когда игровой цикл жив.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
    CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/6000' || exit 1
CMD ["/app/server"]
