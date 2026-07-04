//! Звук: короткие SFX по игровым событиям + фоновая музыка.
//!
//! Подход «как со спрайт-листами, только уже нарезано»: длинные дорожки заранее
//! порезаны на отдельные клипы (см. `assets/audio/...`), а здесь мы просто держим
//! пулы готовых `Handle<AudioSource>` и на каждое событие проигрываем СЛУЧАЙНЫЙ
//! клип из нужного пула. Каждый звук — отдельная сущность `AudioPlayer` в режиме
//! `Despawn` (сама исчезает по окончании), поэтому накладываются без ручного учёта.
//!
//! Раскладка пулов (по просьбе дизайна):
//! - `growls`     — рандомные рыки зомби/скелетов (амбиент, серверный авторитет);
//! - `attacks`    — звук АТАКИ неписи (замах/удар); файлы добавятся позже;
//! - `deaths`     — озвучка смерти неписей;
//! - `flesh`      — удар меча «по плоти» → когда бьём НЕПИСЯ;
//! - `body`       — удар «просто/по броне» → когда бьём ИГРОКА (урон > 0);
//! - `block`      — удар пришёлся в ЩИТ (урон поглощён, damage == 0);
//! - `swing`      — взмах меча (без привязки к попаданию);
//! - `stun`       — удар щитом (стан);
//! - `dash`       — перекат (рывок);
//! - `explosions` — взрывы гранат.
//!
//! Позиционность: рыки/атаки неписей и боевые звуки ЧУЖИХ игроков затухают по
//! дистанции до локального игрока (`dist_volume`) — на слух понятно, далеко ли
//! источник. Рыки/атаки неписей сервер шлёт даже из-за стены (в обход куллинга
//! видимости), так что невидимую угрозу слышно. Свои действия приходят с позицией
//! локального игрока → звучат в полную громкость.
//!
//! Музыка `wilderness` играет тихо в фоне, пока мы «на карте» (state `InGame`).

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use protocol::constants::NPC_AUDIO_RADIUS;
use protocol::messages::NpcSoundKind;

use crate::components::LocalPlayer;
use crate::events::{
    CombatSfxEvent, CombatSfxKind, GrenadeDetonatedEvent, NpcDiedEvent, NpcSoundEvent,
    PlayerDamagedEvent,
};
use crate::render::WorldPos;
use crate::systems::npc::NpcInfo;

/// Громкость SFX и музыки (линейная). Музыку держим тихо, чтобы не перебивала бой.
const SFX_VOLUME: f32 = 0.9;
const HIT_VOLUME: f32 = 0.8;
const BLOCK_VOLUME: f32 = 0.8;
const SWING_VOLUME: f32 = 0.55;
const STUN_VOLUME: f32 = 0.9;
const DASH_VOLUME: f32 = 0.6;
/// Взрыв «потише в 1.5 раза» (было 1.0).
const EXPLOSION_VOLUME: f32 = 0.67;
const MUSIC_VOLUME: f32 = 0.16;

/// Радиус слышимости боевых звуков ЧУЖИХ игроков (мир. ед.): дальше — не слышно.
const COMBAT_HEAR_RADIUS: f32 = 1200.0;

/// Пулы готовых клипов. Грузим один раз на старте.
#[derive(Resource, Default)]
pub struct Sfx {
    pub growls: Vec<Handle<AudioSource>>,
    pub attacks: Vec<Handle<AudioSource>>,
    pub deaths: Vec<Handle<AudioSource>>,
    pub flesh: Vec<Handle<AudioSource>>,
    pub body: Vec<Handle<AudioSource>>,
    pub block: Vec<Handle<AudioSource>>,
    pub swing: Vec<Handle<AudioSource>>,
    pub stun: Vec<Handle<AudioSource>>,
    pub dash: Vec<Handle<AudioSource>>,
    pub explosions: Vec<Handle<AudioSource>>,
    /// Фоновые композиции — крутятся по кругу (wilderness → tombs → ...).
    pub music: Vec<Handle<AudioSource>>,
}

/// Индекс текущей фоновой композиции в `Sfx::music` (переживает выход/вход на
/// карту, чтобы плейлист продолжался с того же места).
#[derive(Resource, Default)]
pub struct BgMusicIdx(pub usize);

/// Простой быстрый ГСЧ (xorshift) — без внешних крейтов; нужен лишь для выбора
/// случайного клипа, криптостойкость не требуется.
#[derive(Resource)]
pub struct AudioRng(u64);

impl Default for AudioRng {
    fn default() -> Self {
        let seed = crate::platform::now_nanos() ^ crate::platform::random_u64() | 1;
        Self(seed)
    }
}

impl AudioRng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Случайный индекс в диапазоне `[0, len)`.
    fn index(&mut self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            (self.next_u64() % len as u64) as usize
        }
    }
}

/// Маркер фоновой музыки — чтобы выключить её при выходе с карты.
#[derive(Component)]
pub struct BgMusic;

/// Грузит все клипы в пулы (Startup).
pub fn setup_audio(mut commands: Commands, asset: Res<AssetServer>) {
    fn load_many(asset: &AssetServer, paths: &[&str]) -> Vec<Handle<AudioSource>> {
        // p.to_string() → AssetPath<'static>, иначе путь «занимает» время жизни слайса
        paths.iter().map(|p| asset.load(p.to_string())).collect()
    }
    commands.insert_resource(Sfx {
        growls: load_many(
            &asset,
            &[
                "audio/npc/growl_1.ogg",
                "audio/npc/growl_2.ogg",
                "audio/npc/growl_3.ogg",
                "audio/npc/growl_4.ogg",
                "audio/npc/growl_5.ogg",
            ],
        ),
        // Звук атаки неписей (панчи): нарезаны из universfield power-punch /
        // fist-fight / punch-02. Шлёт сервер (`NpcSound::Attack`), затухает по
        // дистанции — слышно и из-за стены.
        attacks: load_many(
            &asset,
            &[
                "audio/npc/attack_1.ogg",
                "audio/npc/attack_2.ogg",
                "audio/npc/attack_3.ogg",
            ],
        ),
        deaths: load_many(&asset, &["audio/npc/death_1.ogg", "audio/npc/death_2.ogg"]),
        flesh: load_many(&asset, &["audio/hit/flesh_1.ogg", "audio/hit/flesh_2.ogg"]),
        body: load_many(
            &asset,
            &[
                "audio/hit/body_1.ogg",
                "audio/hit/body_2.ogg",
                "audio/hit/body_3.ogg",
            ],
        ),
        block: load_many(
            &asset,
            &[
                "audio/block/block_1.ogg",
                "audio/block/block_2.ogg",
                "audio/block/block_3.ogg",
            ],
        ),
        swing: load_many(
            &asset,
            &[
                "audio/swing/swipe_1.ogg",
                "audio/swing/swipe_2.ogg",
                "audio/swing/swipe_3.ogg",
                "audio/swing/swipe_4.ogg",
                "audio/swing/swipe_5.ogg",
                "audio/swing/swipe_6.ogg",
                "audio/swing/swipe_7.ogg",
            ],
        ),
        stun: load_many(
            &asset,
            &[
                "audio/stun/shield_1.ogg",
                "audio/stun/shield_2.ogg",
                "audio/stun/shield_3.ogg",
                "audio/stun/shield_4.ogg",
                "audio/stun/shield_5.ogg",
            ],
        ),
        dash: load_many(&asset, &["audio/dash/dash_1.ogg"]),
        explosions: load_many(
            &asset,
            &["audio/explosion/boom_1.ogg", "audio/explosion/boom_2.ogg"],
        ),
        music: vec![
            asset.load("audio/music/wilderness.ogg"),
            asset.load("audio/music/tombs.ogg"),
        ],
    });
}

/// Спавнит одноразовый звук (сам деспавнится по окончании).
fn play_oneshot(commands: &mut Commands, clip: Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            ..default()
        },
    ));
}

/// Проигрывает СЛУЧАЙНЫЙ клип из пула на заданной громкости.
fn play_random(commands: &mut Commands, pool: &[Handle<AudioSource>], rng: &mut AudioRng, vol: f32) {
    if pool.is_empty() || vol <= 0.0 {
        return;
    }
    let i = rng.index(pool.len());
    play_oneshot(commands, pool[i].clone(), vol);
}

/// Громкость источника `src` для слушателя в `listener`: линейно затухает к нулю
/// на границе `radius`. Если слушателя нет (игрок мёртв/не заспавнен) — слышим
/// тихо «отовсюду». `None` — слишком далеко, не играем.
fn dist_volume(src: Vec2, listener: Option<Vec2>, radius: f32, base: f32) -> Option<f32> {
    match listener {
        Some(l) => {
            let d = (src - l).length();
            if d >= radius {
                return None;
            }
            let v = base * (1.0 - d / radius);
            if v < 0.03 {
                None
            } else {
                Some(v)
            }
        }
        None => Some(base * 0.4),
    }
}

/// Позиция локального игрока (слушатель) — для затухания позиционных звуков.
fn listener_pos(q: &Query<&WorldPos, With<LocalPlayer>>) -> Option<Vec2> {
    q.iter().next().map(|w| w.0)
}

/// Удар по ИГРОКУ: урон > 0 → «по телу/броне»; урон == 0 → попал в ЩИТ (блок).
pub fn play_player_hit_sfx(
    mut commands: Commands,
    mut ev: MessageReader<PlayerDamagedEvent>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
) {
    let mut hit = false;
    let mut blocked = false;
    for e in ev.read() {
        if e.damage > 0 {
            hit = true;
        } else {
            blocked = true;
        }
    }
    if hit {
        play_random(&mut commands, &sfx.body, &mut rng, HIT_VOLUME);
    }
    if blocked {
        play_random(&mut commands, &sfx.block, &mut rng, BLOCK_VOLUME);
    }
}

/// Удар по НЕПИСИ → «по плоти». Своего события урона по неписям нет, поэтому
/// ловим УМЕНЬШЕНИЕ hp в `NpcInfo` (он обновляется из снапшотов): hp у неписей
/// только падает, так что любое снижение = попадание.
pub fn play_npc_hit_sfx(
    mut commands: Commands,
    info: Res<NpcInfo>,
    mut prev_hp: Local<std::collections::HashMap<u32, i32>>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
) {
    // Считаем КАЖДУЮ непись, у которой за этот кадр упало hp, и проигрываем по
    // звуку на каждое попадание (клипы — отдельные сущности, накладываются). Так
    // быстрые/множественные удары больше не «съедаются» одним звуком на кадр.
    let mut hits = 0u32;
    for (&id, &(hp, _, _)) in info.0.iter() {
        if let Some(&old) = prev_hp.get(&id) {
            if hp < old {
                hits += 1;
            }
        }
        prev_hp.insert(id, hp);
    }
    // забываем тех, кто пропал из вида/умер, чтобы при повторном появлении не
    // звякнуть ложно
    prev_hp.retain(|id, _| info.0.contains_key(id));

    // кэп, чтобы при массовом уроне (граната по толпе) не было звуковой каши
    for _ in 0..hits.min(3) {
        play_random(&mut commands, &sfx.flesh, &mut rng, HIT_VOLUME);
    }
}

/// Радиус, в котором смерть неписи трактуется как «наш добивающий удар» и звучит
/// «по плоти». Добивают вплотную (melee), так что хватает небольшого радиуса.
const NPC_KILL_FLESH_RADIUS: f32 = 220.0;

/// Смерть неписи → предсмертная озвучка (затухает по дистанции, слышно и из-за
/// стены: `NpcDied` шлётся всем) + удар «по плоти» для ДОБИВАЮЩЕГО удара: его hp
/// уходит в минус и непись пропадает из снапшота раньше, чем `play_npc_hit_sfx`
/// успеет заметить падение hp, поэтому добивание иначе оставалось без звука удара.
pub fn play_npc_death_sfx(
    mut commands: Commands,
    mut ev: MessageReader<NpcDiedEvent>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
    listener: Query<&WorldPos, With<LocalPlayer>>,
) {
    let lpos = listener_pos(&listener);
    for e in ev.read() {
        // удар по плоти — только для близких смертей (наш/рядом добивающий удар)
        if let Some(v) = dist_volume(e.pos, lpos, NPC_KILL_FLESH_RADIUS, HIT_VOLUME) {
            play_random(&mut commands, &sfx.flesh, &mut rng, v);
        }
        // предсмертный хрип — слышно дальше и сквозь стены
        if let Some(v) = dist_volume(e.pos, lpos, NPC_AUDIO_RADIUS, SFX_VOLUME) {
            play_random(&mut commands, &sfx.deaths, &mut rng, v);
        }
    }
}

/// Позиционные звуки неписей (рык/атака) из серверных `NpcSound` — затухают по
/// дистанции, чтобы на слух оценивать, далеко ли (невидимая) угроза.
pub fn play_npc_sound_sfx(
    mut commands: Commands,
    mut ev: MessageReader<NpcSoundEvent>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
    listener: Query<&WorldPos, With<LocalPlayer>>,
) {
    let lpos = listener_pos(&listener);
    for e in ev.read() {
        let (pool, base): (&[Handle<AudioSource>], f32) = match e.kind {
            NpcSoundKind::Growl => (&sfx.growls, SFX_VOLUME),
            NpcSoundKind::Attack => (&sfx.attacks, HIT_VOLUME),
        };
        if let Some(v) = dist_volume(e.pos, lpos, NPC_AUDIO_RADIUS, base) {
            play_random(&mut commands, pool, &mut rng, v);
        }
    }
}

/// Боевые звуки с позицией (взмах/стан/перекат). Свои действия приходят с позицией
/// локального игрока → полная громкость; чужие затухают по дистанции.
pub fn play_combat_sfx(
    mut commands: Commands,
    mut ev: MessageReader<CombatSfxEvent>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
    listener: Query<&WorldPos, With<LocalPlayer>>,
) {
    let lpos = listener_pos(&listener);
    for e in ev.read() {
        let (pool, base): (&[Handle<AudioSource>], f32) = match e.kind {
            CombatSfxKind::Swing => (&sfx.swing, SWING_VOLUME),
            CombatSfxKind::StunBash => (&sfx.stun, STUN_VOLUME),
            CombatSfxKind::Dash => (&sfx.dash, DASH_VOLUME),
            CombatSfxKind::Blocked => (&sfx.block, BLOCK_VOLUME),
        };
        if let Some(v) = dist_volume(e.pos, lpos, COMBAT_HEAR_RADIUS, base) {
            play_random(&mut commands, pool, &mut rng, v);
        }
    }
}

/// Взрыв гранаты → случайный «бум».
pub fn play_explosion_sfx(
    mut commands: Commands,
    mut ev: MessageReader<GrenadeDetonatedEvent>,
    sfx: Res<Sfx>,
    mut rng: ResMut<AudioRng>,
) {
    let mut boom = false;
    for _ in ev.read() {
        boom = true;
    }
    if boom {
        play_random(&mut commands, &sfx.explosions, &mut rng, EXPLOSION_VOLUME);
    }
}

/// Спавнит текущую композицию плейлиста в режиме `Despawn` (НЕ `Loop`): когда она
/// доиграет, сущность исчезнет, и `advance_bg_music` запустит следующую по кругу.
fn spawn_bg_track(commands: &mut Commands, sfx: &Sfx, idx: usize) {
    if let Some(track) = sfx.music.get(idx) {
        commands.spawn((
            AudioPlayer(track.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(MUSIC_VOLUME),
                ..default()
            },
            BgMusic,
        ));
    }
}

/// Включает фоновую музыку при входе на карту (после авторизации = state InGame):
/// запускает текущую композицию плейлиста.
pub fn start_bg_music(
    mut commands: Commands,
    sfx: Res<Sfx>,
    idx: Res<BgMusicIdx>,
    existing: Query<(), With<BgMusic>>,
) {
    // на случай повторного входа не плодим вторую дорожку
    if existing.iter().next().is_some() {
        return;
    }
    spawn_bg_track(&mut commands, &sfx, idx.0);
}

/// Крутит плейлист по кругу: как только текущая дорожка доиграла (сущность
/// `BgMusic` исчезла), переключаемся на следующую (wilderness → tombs → ...).
pub fn advance_bg_music(
    mut commands: Commands,
    sfx: Res<Sfx>,
    mut idx: ResMut<BgMusicIdx>,
    playing: Query<(), With<BgMusic>>,
) {
    if playing.iter().next().is_some() || sfx.music.is_empty() {
        return;
    }
    idx.0 = (idx.0 + 1) % sfx.music.len();
    spawn_bg_track(&mut commands, &sfx, idx.0);
}

/// Глушит музыку при выходе с карты (в меню).
pub fn stop_bg_music(mut commands: Commands, q: Query<Entity, With<BgMusic>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}
