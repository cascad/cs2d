# ⚔ CS2D Bevy Clone

Мультиплеерная 2D-игра с видом сверху, реализованная на Rust с использованием [Bevy 0.16.1](https://bevyengine.org/).

---

## 🧱 Зависимости

* Rust (nightly или stable)
* [cargo-make](https://sagiegurari.github.io/cargo-make/) (opcional)
* Git
* OpenGL/Сompatible GPU

---

## 📦 Сборка

```bash
git clone https://github.com/ca5cad/cs2d.git
cd your-repo-name
cargo build --release
```

---

## 🚀 Запуск

В двух отдельных терминалах:

### 1. Сервер

```bash
cargo run --bin server
```

### 2. Клиент

```bash
cargo run --bin client
```

---

## 🎮 Управление

| Клавиша       | Действие       |
| ------------- | -------------- |
| W / A / S / D | Движение       |
| ЛКМ           | Стрельба       |
| ПКМ / G       | Бросок гранаты |

---

## 🛠 Структура проекта

```
.
├── client/             # Клиент
├── server/             # Сервер
├── protocol/           # Общие сообщения, типы, константы
├── assets/             # Шрифты, текстуры (esli est)
└── Cargo.toml
```

---

## 🔧 Отладка

```bash
RUST_LOG=info cargo run --bin client

# сервер
RUST_LOG=debug cargo run --bin server
```

---

## 📌 Особенности

* Клиент/сервер на bevy\_quinnet
* Лаг-компенсейшн для стрельбы
* Урон, попапы, гранаты, HP UI

---

## 📜 Лицензия

MIT © ca5cad
