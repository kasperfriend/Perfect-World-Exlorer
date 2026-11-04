//! Authentic character-action SFX (`EventType: 101`) from `sfx.pck`.
//!
//! The animation clock owns event timing. This module selects an authored file,
//! applies its authored volume/pitch ranges, and plays it through the existing
//! Windows rodio backend. Random variation selection and non-spatial local
//! playback are Explorer behavior, not a claim of bit-identical client mixing.

use crate::{music, pck};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const BODY_CHANNEL: u8 = 0;
pub const MOUNT_CHANNEL: u8 = 1;
pub const FLIGHT_CHANNEL: u8 = 2;

#[derive(Clone, Debug)]
pub struct SoundCue {
    pub channel: u8,
    pub event: u64,
    pub paths: Vec<String>,
    pub volume_min: u32,
    pub volume_max: u32,
    pub pitch_min: f32,
    pub pitch_max: f32,
    pub percent: f32,
    pub looping: bool,
}

#[derive(Clone, Debug)]
pub enum SoundCommand {
    Play(SoundCue),
    Stop { channel: u8, event: u64 },
    StopChannel(u8),
    StopAll,
}

#[derive(Clone, Debug)]
enum Asset {
    Packed(pck::Entry),
    Loose(PathBuf),
}

#[derive(Clone, Debug)]
struct Play {
    channel: u8,
    event: u64,
    label: String,
    asset: Asset,
    gain: f32,
    speed: f32,
    looping: bool,
}

#[derive(Clone, Debug)]
enum WorkerCommand {
    Play(Play),
    Stop { channel: u8, event: u64 },
    StopChannel(u8),
    StopAll,
}

struct Request {
    enabled: bool,
    volume: f32,
    commands: Vec<WorkerCommand>,
}

pub struct CharacterAudio {
    element: PathBuf,
    packed: HashMap<String, pck::Entry>,
    resolved: HashMap<String, Option<Asset>>,
    catalog_error: Option<String>,
    worker_error: Option<String>,
    missing: HashSet<String>,
    status: String,
    enabled: bool,
    volume: f32,
    cues: u64,
    random: u32,
    last_update: Instant,
    last_enabled: bool,
    last_volume: f32,
    #[cfg(windows)]
    tx: Option<std::sync::mpsc::Sender<Request>>,
    #[cfg(windows)]
    rx: Option<std::sync::mpsc::Receiver<Option<String>>>,
}

impl Default for CharacterAudio {
    fn default() -> Self {
        Self {
            element: PathBuf::new(),
            packed: HashMap::new(),
            resolved: HashMap::new(),
            catalog_error: None,
            worker_error: None,
            missing: HashSet::new(),
            status: "Character sounds off".into(),
            enabled: false,
            volume: 0.5,
            cues: 0,
            random: 0x51f15eED,
            last_update: Instant::now() - Duration::from_secs(1),
            last_enabled: false,
            last_volume: -1.,
            #[cfg(windows)]
            tx: None,
            #[cfg(windows)]
            rx: None,
        }
    }
}

impl CharacterAudio {
    pub fn new(element: &Path) -> Self {
        let catalog = std::fs::File::open(element.join("sfx.pck"))
            .map_err(|e| e.to_string())
            .and_then(|mut file| pck::directory(&mut file));
        let catalog_error = catalog.as_ref().err().cloned();
        let packed = catalog
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.name.to_lowercase(), entry))
            .collect();
        #[cfg(windows)]
        let (tx, rx) = {
            let (tx, input) = std::sync::mpsc::channel();
            let (output, rx) = std::sync::mpsc::channel();
            let archive = element.join("sfx.pck");
            std::thread::Builder::new()
                .name("walker-sounds".into())
                .spawn(move || worker(input, output, archive))
                .expect("character sound worker");
            (Some(tx), Some(rx))
        };
        Self {
            element: element.into(),
            packed,
            resolved: HashMap::new(),
            catalog_error,
            status: "Character action sounds ready".into(),
            #[cfg(windows)]
            tx,
            #[cfg(windows)]
            rx,
            ..Self::default()
        }
    }

    fn random(&mut self) -> u32 {
        // A local deterministic stream avoids adding a global RNG dependency.
        // Native file/TTL/random-sequence parity is not claimed.
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        x
    }

    fn resolve(&mut self, path: &str) -> Option<Asset> {
        if let Some(asset) = self.resolved.get(path) {
            return asset.clone();
        }
        let asset = self
            .packed
            .get(path)
            .cloned()
            .map(Asset::Packed)
            .or_else(|| music::resolve_under(&self.element, path, "sfx").map(Asset::Loose));
        self.resolved.insert(path.into(), asset.clone());
        asset
    }

    fn prepare(&mut self, cue: SoundCue) -> Option<WorkerCommand> {
        if cue.paths.is_empty() || self.random() as f32 / u32::MAX as f32 > cue.percent {
            return None;
        }
        let path_index = self.random() as usize % cue.paths.len();
        let label = cue.paths[path_index].clone();
        let asset = self.resolve(&label);
        if asset.is_none() {
            self.missing.insert(label);
            return None;
        }
        let volume_range = cue.volume_max.saturating_sub(cue.volume_min);
        let volume = if volume_range == 0 {
            cue.volume_min
        } else {
            cue.volume_min + self.random() % (volume_range + 1)
        };
        let pitch_unit = self.random() as f32 / u32::MAX as f32;
        let pitch = cue.pitch_min + (cue.pitch_max - cue.pitch_min) * pitch_unit;
        self.cues = self.cues.wrapping_add(1);
        Some(WorkerCommand::Play(Play {
            channel: cue.channel,
            event: cue.event,
            label,
            asset: asset?,
            gain: volume as f32 / 100.,
            speed: 2.0f32.powf(pitch / 12.),
            looping: cue.looping,
        }))
    }

    pub fn update(&mut self, enabled: bool, volume: f32, commands: Vec<SoundCommand>) {
        self.enabled = enabled;
        self.volume = volume.clamp(0., 1.);
        #[cfg(windows)]
        if let Some(rx) = &self.rx {
            while let Ok(message) = rx.try_recv() {
                if enabled {
                    self.worker_error = message;
                }
            }
        }
        if !enabled {
            self.worker_error = None;
        }
        let empty = commands.is_empty();
        if empty
            && enabled == self.last_enabled
            && (self.volume - self.last_volume).abs() < 0.001
            && self.last_update.elapsed() < Duration::from_millis(250)
        {
            return;
        }
        self.last_update = Instant::now();
        self.last_enabled = enabled;
        self.last_volume = self.volume;
        self.status = if enabled {
            format!("Character action sounds on; {} cue(s) triggered", self.cues)
        } else {
            "Character sounds off".into()
        };
        #[cfg(not(windows))]
        if enabled {
            self.status.push_str(" (audio playback is Windows-only)");
        }
        let commands: Vec<WorkerCommand> = if enabled {
            #[cfg(windows)]
            {
                commands
                    .into_iter()
                    .filter_map(|command| match command {
                        SoundCommand::Play(cue) => self.prepare(cue),
                        SoundCommand::Stop { channel, event } => {
                            Some(WorkerCommand::Stop { channel, event })
                        }
                        SoundCommand::StopChannel(channel) => {
                            Some(WorkerCommand::StopChannel(channel))
                        }
                        SoundCommand::StopAll => Some(WorkerCommand::StopAll),
                    })
                    .collect()
            }
            #[cfg(not(windows))]
            {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(Request {
                enabled,
                volume: self.volume,
                commands,
            });
        }
        #[cfg(not(windows))]
        let _ = commands;
    }

    pub fn stop(&mut self) {
        self.update(self.enabled, self.volume, vec![SoundCommand::StopAll]);
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn diagnostic(&self) -> Option<String> {
        let mut messages = Vec::new();
        if let Some(error) = &self.catalog_error {
            messages.push(format!("sfx.pck unavailable: {error}"));
        }
        if let Some(error) = &self.worker_error {
            messages.push(error.clone());
        }
        if !self.missing.is_empty() {
            let mut paths = self.missing.iter().cloned().collect::<Vec<_>>();
            paths.sort();
            messages.push(format!(
                "Unresolved character sound(s): {}",
                paths.join(", ")
            ));
        }
        if messages.is_empty() {
            None
        } else {
            Some(messages.join("; "))
        }
    }
}

/// SFX paths are relative to the SFX root, just like native `"Sfx\\" + path`.
pub fn asset_key(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
    {
        return None;
    }
    let path = path.to_lowercase();
    Some(if path.starts_with("sfx/") {
        path
    } else {
        format!("sfx/{path}")
    })
}

#[cfg(windows)]
fn worker(
    rx: std::sync::mpsc::Receiver<Request>,
    status: std::sync::mpsc::Sender<Option<String>>,
    archive: PathBuf,
) {
    use rodio::{Decoder, OutputStream, Sink, Source};
    use std::{
        io::{BufReader, Cursor},
        sync::Arc,
    };

    struct Playing {
        sink: Sink,
        gain: f32,
    }
    let mut output = None;
    let mut retry_output = Instant::now();
    let mut active: HashMap<(u8, u64), Playing> = HashMap::new();
    let mut cache: HashMap<String, Arc<[u8]>> = HashMap::new();
    let mut cached_bytes = 0usize;
    let mut failed = HashSet::<String>::new();
    loop {
        let mut request = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(request) => request,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
        };
        while let Ok(next) = rx.try_recv() {
            // Volume/enabled state is latest-wins, but animation commands are
            // edges. Coalescing whole requests would silently discard steps,
            // stops, and travel-channel cleanup produced by earlier frames.
            request.commands.extend(next.commands);
            request.enabled = next.enabled;
            request.volume = next.volume;
        }
        active.retain(|_, playing| !playing.sink.empty());
        if !request.enabled {
            for (_, playing) in active.drain() {
                playing.sink.stop();
            }
            failed.clear();
            continue;
        }
        if output.is_none() && Instant::now() >= retry_output {
            match OutputStream::try_default() {
                Ok(stream) => output = Some(stream),
                Err(error) => {
                    let _ = status.send(Some(format!(
                        "Character sound output unavailable: {error}"
                    )));
                    retry_output = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        let mut had_error = false;
        for command in request.commands {
            match command {
                WorkerCommand::Stop { channel, event } => {
                    if let Some(playing) = active.remove(&(channel, event)) {
                        playing.sink.stop();
                    }
                }
                WorkerCommand::StopChannel(channel) => {
                    active.retain(|(owner, _), playing| {
                        if *owner == channel {
                            playing.sink.stop();
                            false
                        } else {
                            true
                        }
                    });
                }
                WorkerCommand::StopAll => {
                    for (_, playing) in active.drain() {
                        playing.sink.stop();
                    }
                }
                WorkerCommand::Play(play) => {
                    if failed.contains(&play.label) {
                        continue;
                    }
                    if active.contains_key(&(play.channel, play.event)) {
                        continue;
                    }
                    let Some((_, handle)) = output.as_ref() else {
                        continue;
                    };
                    let bytes = if let Some(bytes) = cache.get(&play.label) {
                        Ok(bytes.clone())
                    } else {
                        let read = match &play.asset {
                            Asset::Packed(entry) => std::fs::File::open(&archive)
                                .map_err(|e| e.to_string())
                                .and_then(|mut file| pck::read_entry(&mut file, entry)),
                            Asset::Loose(path) => {
                                std::fs::read(path).map_err(|e| e.to_string())
                            }
                        };
                        read.map(|bytes| {
                            let bytes: Arc<[u8]> = bytes.into();
                            if cached_bytes + bytes.len() > 16 * 1024 * 1024 {
                                cache.clear();
                                cached_bytes = 0;
                            }
                            if bytes.len() <= 16 * 1024 * 1024 {
                                cached_bytes += bytes.len();
                                cache.insert(play.label.clone(), bytes.clone());
                            }
                            bytes
                        })
                    };
                    match bytes.and_then(|bytes| {
                        Decoder::new(BufReader::new(Cursor::new(bytes)))
                            .map_err(|e| e.to_string())
                    }) {
                        Ok(source) => match Sink::try_new(handle) {
                            Ok(sink) => {
                                sink.set_volume((request.volume * play.gain).clamp(0., 1.));
                                sink.set_speed(play.speed.max(0.01));
                                if play.looping {
                                    sink.append(source.repeat_infinite());
                                } else {
                                    sink.append(source);
                                }
                                active.insert(
                                    (play.channel, play.event),
                                    Playing {
                                        sink,
                                        gain: play.gain,
                                    },
                                );
                            }
                            Err(error) => {
                                had_error = true;
                                let _ = status.send(Some(format!(
                                    "Character sound output error: {error}"
                                )));
                            }
                        },
                        Err(error) => {
                            had_error = true;
                            failed.insert(play.label.clone());
                            let _ = status.send(Some(format!(
                                "Cannot load character sound {}: {error}",
                                play.label
                            )));
                        }
                    }
                }
            }
        }
        for playing in active.values() {
            playing
                .sink
                .set_volume((request.volume * playing.gain).clamp(0., 1.));
        }
        if !had_error {
            let _ = status.send(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentic_sfx_paths_are_rooted_and_traversal_is_rejected() {
        assert_eq!(
            asset_key("Effect\\Player\\Step01.wav").as_deref(),
            Some("sfx/effect/player/step01.wav")
        );
        assert_eq!(
            asset_key("SFX/Effect/Swim.ogg").as_deref(),
            Some("sfx/effect/swim.ogg")
        );
        for path in ["../x.wav", "/x.wav", "C:/x.wav", "a//b.wav"] {
            assert_eq!(asset_key(path), None, "accepted {path}");
        }
    }
}
