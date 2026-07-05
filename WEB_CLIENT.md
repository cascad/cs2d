# Браузерный клиент (WASM + WebTransport)

## Быстрый старт (локально)

1. Установить toolchain:
   ```powershell
   rustup target add wasm32-unknown-unknown
   cargo install trunk wasm-bindgen-cli
   ```

2. Запустить сервер (печатает **certificate digest** и пишет `certificates/digest.txt`):
   ```powershell
   cargo run -p server
   ```

3. Пересобрать wasm (digest вшивается из `certificates/digest.txt`):
   ```powershell
   cd client
   trunk serve
   ```
   Release-профиль включён в `Trunk.toml` (`[build] release = true`) — отдельный
   `trunk build --release` не нужен, а `trunk serve` больше НЕ перезаписывает
   сборку debug-версией (debug-wasm не тянет тикрейт: движение начинает
   «плавать» с многосекундной задержкой и дёргаться).

4. Открыть `http://127.0.0.1:8080/?server=127.0.0.1:6001`

   Digest подтягивается автоматически из `/certificates/digest.txt` (копируется Trunk'ом).
   Если сервер перезапускали — достаточно **F5** в браузере (пересборка wasm не нужна).
   Ручной override: `#<digest_из_лога_сервера>` в URL.

## Прод (VPS + домен)

### Схема

Домен нужен ТОЛЬКО странице (secure context для WebTransport). Игровой сокет
подключается **по IP** с hash-валидацией самоподписанного сертификата — как в
dev. Почему не Let's Encrypt на игровом порту:

- lightyear 0.26 строит URL подключения из `SocketAddr` (`https://IP:6001`) —
  доменное имя туда не подставить, а LE не выдаёт сертификаты на IP;
- Chrome принимает `serverCertificateHashes` только для сертификатов со сроком
  жизни ≤ 14 дней — LE (90 дней) через хеши не пройдёт в принципе;
- наш wasm-клиент и так тянет живой digest по HTTP при каждом F5.

Self-signed сертификат живёт **14 дней** (лимит wtransport = лимит Chrome) и
пересоздаётся при каждом старте сервера ⇒ **рестарт сервера минимум раз в
~10-13 дней** (systemd-таймер ниже; после рестарта игрокам достаточно F5).

### DNS (Cloudflare)

Как для любого сервиса: A-запись `game.example.com → IP VPS`, но тучка
обязательно **серая (DNS only)**. Оранжевый прокси Cloudflare не пропускает
произвольный UDP — ни WebTransport :6001 (QUIC), ни нативный :6000. Разные
домены для статики и :6001 НЕ нужны: страница ходит на 443/tcp по имени, игра —
на 6001/udp по IP из `?server=`.

### Вариант A: docker compose (рекомендуется) — каталог `deploy/`

Три контейнера: `game` (headless-сервер, multi-stage сборка из исходников),
`web` (Caddy: статика + автоматический Let's Encrypt) и `restarter`
(еженедельный рестарт `game` — свежий self-signed сертификат). Живой
`certificates/digest.txt` игрового сервера прокинут в Caddy общим томом —
браузер всегда получает актуальный digest.

```bash
# на VPS
git clone <репозиторий> /opt/cs2d && cd /opt/cs2d/deploy
cp .env.example .env            # вписать DOMAIN
docker compose up -d --build

# на машине разработчика (wasm в docker не собираем — тяжёлый toolchain)
cd client && trunk build
# ЛОВУШКА scp: если целевой каталог уже существует, scp кладёт копию ВНУТРЬ
# него (~/dist/dist/) — поэтому сначала сносим цель:
ssh vps "rm -rf ~/dist"
scp -r ../dist vps:~/dist
ssh -t vps "sudo rsync -a --delete ~/dist/ /opt/cs2d/dist/"
```

Обновление: `git pull && docker compose up -d --build` + свежий `dist/`.

#### Порт 80 занят (на VPS уже живёт другой сайт)

Наш Caddy не нужен — TLS-терминатором остаётся существующий веб-сервер.
Поднимаем только игру (без сервиса `web`):

```bash
cd /opt/cs2d/deploy
docker compose rm -sf web              # убрать недостартовавший контейнер
docker compose up -d --build game restarter
```

Статику отдаёт существующий nginx (`certbot --nginx -d cs2d.example.com`
добавит TLS-блок сам):

```nginx
server {
    listen 80;
    server_name cs2d.example.com;
    root /opt/cs2d/dist;

    # голая ссылка → редирект с адресом игрового сокета (IP свой)
    location = / {
        if ($arg_server = "") { return 302 /?server=<IP_VPS>:6001; }
        try_files /index.html =404;
    }

    # ЖИВОЙ digest сертификата игрового сервера (том compose-сервиса game)
    location /certificates/ {
        alias /opt/cs2d/deploy/data/certificates/;
    }
}
```

Проверить, что wasm отдаётся с типом `application/wasm`: в свежих nginx он в
`mime.types` из коробки (`curl -I .../client-*.wasm`).

### Вариант B: вручную (nginx + certbot + systemd)

1. **Сервер** (headless, ассеты не нужны): собрать `cargo build --release -p
   server` на VPS (или CI под linux), положить в `/opt/cs2d/`. В
   `server_config.toml`: `ip = "0.0.0.0"`, порты по умолчанию, `tls_*` НЕ
   задавать (self-signed). systemd-юнит:
   ```ini
   [Unit]
   Description=CS2D game server
   After=network.target
   [Service]
   WorkingDirectory=/opt/cs2d
   ExecStart=/opt/cs2d/server
   Restart=always
   RuntimeMaxSec=7d   # рестарт раз в неделю: свежий cert (живёт 14 дней)
   [Install]
   WantedBy=multi-user.target
   ```
   `WorkingDirectory` важен: digest пишется в `/opt/cs2d/certificates/digest.txt`.
2. **Статика**: локально `cd client && trunk build` → залить `dist/` в
   `/opt/cs2d/dist/`. Удалить `dist/certificates/` из выгрузки — digest должен
   отдаваться ЖИВОЙ, из каталога сервера (иначе протухший digest из сборки
   молча ломает TLS-хендшейк).
3. **nginx** (+ `certbot --nginx -d game.example.com`):
   ```nginx
   server {
       server_name game.example.com;
       root /opt/cs2d/dist;
       # живой digest игрового сервера вместо протухшего из сборки
       location /certificates/ { alias /opt/cs2d/certificates/; }
       # wasm обязан отдаваться с правильным MIME
       types { application/wasm wasm; }
       include /etc/nginx/mime.types;
   }
   ```

### Общее для обоих вариантов

- **Firewall**: `443/tcp` + `80/tcp` (ACME) — сайт; `6001/udp` — браузерные
  клиенты; `6000/udp` + `6000/tcp` — нативные клиенты (игра + мета лобби).
- **Ссылка игрокам**: `https://game.example.com/?server=<IP_VPS>:6001`.
- **Вшитый конфиг wasm**: в `client_config.toml` на момент `trunk build` поле
  `name` должно быть ровно `"Player"` (или пустым) — тогда каждому браузерному
  игроку генерится случайное имя. Иначе все игроки по ссылке делят один аккаунт.

Нативный `client.exe` ходит на `<IP_VPS>:6000` (UDP; в `client_config.toml`
адрес указывается IP — доменные имена клиент пока не резолвит). Оба транспорта
на одном сервере, один мир.

Файлы `tls_cert_pem`/`tls_key_pem` в конфиге сервера остаются на будущее: они
пригодятся, если lightyear научится подключаться по имени хоста (тогда можно
перейти на LE и убрать таймер рестартов).

## Ограничения

- WebTransport требует secure context: `localhost` ок; голый `http://IP` с другой машины — нет (нужен HTTPS или флаг Chrome).
- Звук в браузере стартует после первого клика/нажатия клавиши.
- **Firefox не работает**: датаграммы WebTransport в FF не поддерживают BYOB-ридеры,
  которые использует wasm-стек lightyear (`xwt-web`) — ошибка
  `ReadableStream.getReader: Trying to read with incompatible controller`
  ([Bugzilla 2007755](https://bugzilla.mozilla.org/show_bug.cgi?id=2007755),
  [xwt#156](https://github.com/MOZGIII/xwt/issues/156)). Тестировать в **Chrome/Edge**.
- Safari: WebTransport ограничен; позже можно добавить WebSocket-fallback (закроет и Firefox).
