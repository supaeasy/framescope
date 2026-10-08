//! Player-Zustand: Wiedergabeuhr, Frame-Cache, Seeking und Einzelbild-Schritte.
//!
//! Die UI fragt pro Frame `poll()` ab; der Decoder-Thread liefert Frames in
//! Präsentationsreihenfolge. Zeitstempel stammen aus derselben Berechnung wie im
//! Frame-Index und lassen sich daher exakt vergleichen (Toleranz `EPS`).

use crate::audio::AudioEngine;
use crate::decoder::{Command, DecoderHandle, Event, Frame, VideoInfo};
use crate::index::FrameIndex;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

const EPS: f64 = 1e-6;
/// Speicherbudget für den Frame-Cache (rückwärts Schritte ohne neues Decodieren).
const CACHE_BUDGET: usize = 256 * 1024 * 1024;
/// So weit (in Frames) darf der Decoder vorwärts „durchlaufen“, bevor ein Seek günstiger ist.
const MAX_WAIT_FRAMES: usize = 120;

pub struct Player {
    handle: DecoderHandle,
    pub path: PathBuf,
    /// Angezeigte und verworfene (zu spät dekodierte) Frames, für die Performance-Anzeige.
    pub shown: u64,
    pub dropped: u64,
    /// Zähler für Zustandsänderungen (Play/Pause/Seek/Schritt); dient der Instanz-Synchronisierung.
    pub events: u64,
    /// Gemessene Dauer eines Seeks bis zum angezeigten Zielframe (Sekunden, geglättet).
    seek_latency: f64,
    seek_started: Option<Instant>,
    /// Loop-Bereich (Frame-Nummern, inklusive) und Schalter.
    pub loop_in: Option<usize>,
    pub loop_out: Option<usize>,
    pub loop_on: bool,
    pub info: Option<VideoInfo>,
    pub index: Option<Arc<FrameIndex>>,
    /// Keyframe-Positionen als Anteil der Dauer (für die Timeline).
    pub key_fracs: Vec<f32>,
    pub playing: bool,
    pub error: Option<String>,
    pub current: Option<Arc<Frame>>,
    serial: u64,
    base_pos: f64,
    base_time: Instant,
    /// Nach einem Seek/Schritt: der erste Frame mit `pts >= target` wird angezeigt.
    target: Option<f64>,
    pending: Option<Arc<Frame>>,
    /// PTS des zuletzt vom Decoder empfangenen Frames.
    stream_pts: f64,
    /// PTS des zuletzt über den Decoder-Strom angezeigten Frames.
    stream_shown: f64,
    cache: VecDeque<Arc<Frame>>,
    eof: bool,
    audio: Option<AudioEngine>,
    pub volume: f32,
    pub muted: bool,
}

impl Player {
    pub fn new(
        path: PathBuf,
        wake: impl Fn() + Send + Sync + Clone + 'static,
        volume: f32,
        muted: bool,
    ) -> Self {
        let audio = AudioEngine::start(&path, volume, muted);
        Self {
            handle: DecoderHandle::spawn(path.clone(), wake),
            path,
            shown: 0,
            dropped: 0,
            events: 0,
            seek_latency: 0.15,
            seek_started: None,
            loop_in: None,
            loop_out: None,
            loop_on: false,
            info: None,
            index: None,
            key_fracs: Vec::new(),
            playing: true,
            error: None,
            current: None,
            serial: 0,
            base_pos: 0.0,
            base_time: Instant::now(),
            target: Some(f64::NEG_INFINITY),
            pending: None,
            stream_pts: f64::NEG_INFINITY,
            stream_shown: f64::NAN,
            cache: VecDeque::new(),
            eof: false,
            audio,
            volume,
            muted,
        }
    }

    pub fn fps(&self) -> f64 {
        self.info.as_ref().map_or(25.0, |i| i.fps)
    }

    pub fn duration(&self) -> f64 {
        match (&self.info, &self.index) {
            (Some(i), _) if i.duration > 0.0 => i.duration,
            (_, Some(idx)) => idx.last_pts() + 1.0 / self.fps(),
            _ => 0.0,
        }
    }

    /// Aktuelle Wiedergabeposition in Sekunden.
    pub fn position(&self) -> f64 {
        if self.playing && self.target.is_none() {
            // Audio ist Master-Clock; ohne Ton läuft die Systemuhr.
            self.audio
                .as_ref()
                .and_then(AudioEngine::clock)
                .unwrap_or_else(|| self.base_pos + self.base_time.elapsed().as_secs_f64())
        } else {
            self.base_pos
        }
    }

    /// Nummer des angezeigten Frames (0-basiert), sobald der Index vorliegt.
    pub fn frame_no(&self) -> Option<usize> {
        let (idx, cur) = (self.index.as_ref()?, self.current.as_ref()?);
        Some(idx.frame_at(cur.pts))
    }

    pub fn frame_count(&self) -> Option<usize> {
        self.index.as_ref().map(|i| i.len())
    }

    /// Verarbeitet Decoder-Events. Rückgabe: `true`, wenn sich `current` geändert hat.
    pub fn poll(&mut self) -> bool {
        if let Ok(idx) = self.handle.index.try_recv() {
            self.index = Some(idx);
        }
        let mut assigned = 0u64;
        loop {
            let (frame, fresh) = match self.pending.take() {
                Some(f) => (f, false),
                None => match self.handle.events.try_recv() {
                    Ok(Event::Opened(info)) => {
                        self.info = Some(info);
                        continue;
                    }
                    Ok(Event::Frame(f)) => (f, true),
                    Ok(Event::Eof(s)) => {
                        self.eof |= s == self.serial;
                        continue;
                    }
                    Ok(Event::Error(e)) => {
                        self.error = Some(e);
                        break;
                    }
                    Err(_) => break,
                },
            };
            if frame.serial != self.serial {
                continue; // vor einem Seek dekodiert
            }
            if fresh {
                self.stream_pts = frame.pts;
                self.cache_insert(&frame);
            }
            if let Some(t) = self.target {
                if frame.pts < t {
                    continue; // nur für den Cache (Prefetch vor dem Ziel)
                }
                self.target = None;
                if let Some(started) = self.seek_started.take() {
                    let measured = started.elapsed().as_secs_f64();
                    self.seek_latency = (0.7 * self.seek_latency + 0.3 * measured).clamp(0.02, 0.8);
                }
                self.base_pos = frame.pts;
                self.base_time = Instant::now();
            } else {
                let due = self.playing && frame.pts <= self.position();
                if due && self.loop_end_reached(frame.pts) {
                    self.loop_jump();
                    continue;
                }
                if !due {
                    self.pending = Some(frame);
                    break;
                }
            }
            self.stream_shown = frame.pts;
            self.current = Some(frame);
            assigned += 1;
        }
        // Mehrere Frames in einem Durchlauf: nur der letzte wird tatsächlich gezeichnet.
        let changed = assigned > 0;
        self.shown += assigned.min(1);
        self.dropped += assigned.saturating_sub(1);
        self.fill_key_fracs();
        self.sync_audio();
        // Ende erreicht und letzter Frame angezeigt → stoppen.
        if self.eof && self.pending.is_none() && self.playing && self.target.is_none() {
            if self.loop_on {
                self.loop_jump();
            } else {
                self.base_pos = self.current.as_ref().map_or(self.base_pos, |c| c.pts);
                self.playing = false;
                self.events += 1;
            }
        }
        changed
    }

    /// Zeit, ab der die Wiedergabe am Loop-Ende umspringt (`None` = Streamende).
    fn loop_end_time(&self) -> Option<f64> {
        let idx = self.index.as_ref()?;
        idx.pts_of(self.loop_out? + 1)
    }

    /// Ist `pts` (ein fälliger Frame) hinter dem Loop-Ende?
    fn loop_end_reached(&self, pts: f64) -> bool {
        self.loop_on && self.loop_end_time().is_some_and(|end| pts >= end - EPS)
    }

    /// Springt zum Loop-Anfang (oder Videostart) und spielt weiter.
    fn loop_jump(&mut self) {
        let Some(idx) = self.index.clone() else {
            return;
        };
        let start = self.loop_in.unwrap_or(0);
        if let Some(t) = idx.pts_of(start) {
            self.hard_seek(t, t);
        }
    }

    pub fn set_loop_in(&mut self) {
        if let Some(n) = self.frame_no() {
            self.loop_in = Some(n);
            if self.loop_out.is_some_and(|o| o < n) {
                self.loop_out = None;
            }
        }
    }

    pub fn set_loop_out(&mut self) {
        if let Some(n) = self.frame_no() {
            self.loop_out = Some(n);
            if self.loop_in.is_some_and(|i| i > n) {
                self.loop_in = None;
            }
        }
    }

    pub fn clear_loop(&mut self) {
        self.loop_in = None;
        self.loop_out = None;
    }

    /// Loop-Bereich als Anteile der Dauer (nur, wenn mindestens ein Marker gesetzt ist).
    pub fn loop_band(&self) -> Option<(f32, f32)> {
        let idx = self.index.as_ref()?;
        let dur = self.duration();
        if (self.loop_in.is_none() && self.loop_out.is_none()) || dur <= 0.0 {
            return None;
        }
        let start = self.loop_in.and_then(|i| idx.pts_of(i)).unwrap_or(0.0);
        let end = self
            .loop_out
            .and_then(|o| idx.pts_of(o + 1).or(Some(dur)))
            .unwrap_or(dur);
        Some(((start / dur) as f32, (end / dur).min(1.0) as f32))
    }

    fn fill_key_fracs(&mut self) {
        let dur = self.duration();
        if !self.key_fracs.is_empty() || dur <= 0.0 {
            return;
        }
        if let Some(idx) = &self.index {
            self.key_fracs = idx.key_times().map(|t| (t / dur) as f32).collect();
        }
    }

    fn cache_insert(&mut self, frame: &Arc<Frame>) {
        if self.cache.iter().any(|f| (f.pts - frame.pts).abs() < EPS) {
            return;
        }
        self.cache.push_back(frame.clone());
        let cap = self.cache_capacity();
        while self.cache.len() > cap {
            self.cache.pop_front();
        }
    }

    fn cache_capacity(&self) -> usize {
        let bytes = self
            .current
            .as_ref()
            .or(self.cache.back())
            .map_or(1, |f| f.rgba.len().max(1));
        (CACHE_BUDGET / bytes).clamp(2, 48)
    }

    fn cached(&self, pts: f64) -> Option<Arc<Frame>> {
        self.cache
            .iter()
            .find(|f| (f.pts - pts).abs() < EPS)
            .cloned()
    }

    /// Zeigt `frame` an einem Ziel an, ohne den Decoder-Strom zu berühren.
    fn show_cached(&mut self, frame: Arc<Frame>) {
        self.events += 1;
        self.base_pos = frame.pts;
        self.base_time = Instant::now();
        self.target = None;
        self.current = Some(frame);
    }

    /// Ist der Decoder-Strom direkt hinter dem angezeigten Frame positioniert?
    fn synced(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|c| (c.pts - self.stream_shown).abs() < EPS)
    }

    /// Springt per Decoder zu `time`; liefert ab `from` alle Frames (Prefetch für den Cache).
    fn hard_seek(&mut self, time: f64, from: f64) {
        self.events += 1;
        self.seek_started = Some(Instant::now());
        self.serial += 1;
        self.target = Some(time - EPS);
        self.pending = None;
        self.eof = false;
        self.stream_pts = from - 1.0;
        self.base_pos = time;
        self.base_time = Instant::now();
        if let Some(a) = &self.audio {
            a.seek(self.serial, time);
        }
        let _ = self.handle.cmd.send(Command::Seek {
            serial: self.serial,
            from: from - EPS,
        });
    }

    /// Seek auf eine Zeit (z. B. Scrubbing). Mit Index frame-genau.
    pub fn seek_time(&mut self, time: f64) {
        let time = time.clamp(0.0, self.duration().max(0.0));
        let was_playing = self.playing;
        match self.index.clone() {
            Some(idx) if !idx.is_empty() => self.goto_frame(idx.frame_at(time)),
            _ => {
                let tol = 0.5 / self.fps();
                self.hard_seek(time - tol, time - tol);
            }
        }
        if was_playing {
            self.set_playing(true);
        }
    }

    /// Zeigt Frame `n` an (aus dem Cache, durch Weiterlaufen des Decoders oder per Seek).
    pub fn goto_frame(&mut self, n: usize) {
        self.events += 1;
        let Some(idx) = self.index.clone() else {
            return;
        };
        let Some(t) = idx.pts_of(n.min(idx.len().saturating_sub(1))) else {
            return;
        };
        self.set_playing(false);
        if let Some(f) = self.cached(t) {
            self.show_cached(f);
            return;
        }
        // Weiterlaufen lohnt nur, wenn zwischen Decoder-Position und Ziel kein Keyframe liegt
        // (sonst ist ein Seek zum Keyframe schneller).
        if self.stream_pts.is_finite() && t > self.stream_pts && !self.eof {
            let (sf, tf) = (idx.frame_at(self.stream_pts.max(0.0)), idx.frame_at(t));
            let key_between = idx.next_key(sf).is_some_and(|k| k <= tf);
            if !key_between && tf - sf <= MAX_WAIT_FRAMES {
                self.target = Some(t - EPS);
                self.base_pos = t;
                return;
            }
        }
        let start = n.saturating_sub(self.cache_capacity() - 1);
        let from = idx.pts_of(start).unwrap_or(t);
        self.hard_seek(t, from);
    }

    /// Einzelbild vor/zurück.
    pub fn step(&mut self, delta: isize) {
        let (Some(n), Some(count)) = (self.frame_no(), self.frame_count()) else {
            return;
        };
        let new = n.saturating_add_signed(delta).min(count.saturating_sub(1));
        self.goto_frame(new);
    }

    /// `n` Keyframes vor (positiv) bzw. zurück (negativ) springen.
    pub fn step_key_n(&mut self, n: isize) {
        let (Some(mut frame), Some(idx)) = (self.frame_no(), self.index.clone()) else {
            return;
        };
        for _ in 0..n.unsigned_abs() {
            let next = if n > 0 {
                idx.next_key(frame)
            } else {
                idx.prev_key(frame)
            };
            match next {
                Some(k) => frame = k,
                None => break,
            }
        }
        self.goto_frame(frame);
    }

    fn at_end(&self) -> bool {
        match (self.frame_no(), self.frame_count()) {
            (Some(n), Some(count)) => n + 1 >= count,
            _ => self.eof && self.pending.is_none(),
        }
    }

    pub fn set_playing(&mut self, play: bool) {
        if play == self.playing {
            return;
        }
        self.events += 1;
        if let (true, true, false, Some(cur)) = (
            play,
            self.target.is_none(),
            self.synced(),
            self.current.clone(),
        ) {
            // Nach Cache-Schritten steht der Decoder woanders → neu ausrichten.
            self.hard_seek(cur.pts, cur.pts);
        }
        self.base_pos = self.position();
        self.base_time = Instant::now();
        self.playing = play;
        self.sync_audio();
    }

    /// Ton läuft nur, wenn abgespielt wird und der Zielframe bereits angezeigt ist.
    fn sync_audio(&self) {
        if let Some(a) = &self.audio {
            a.set_playing(self.playing && self.target.is_none());
        }
    }

    /// Führt gerade die Audio-Clock die Wiedergabe?
    pub fn audio_is_master(&self) -> bool {
        self.playing
            && self.target.is_none()
            && self.audio.as_ref().is_some_and(|a| a.clock().is_some())
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(a) = &self.audio {
            a.set_volume(self.volume);
        }
    }

    pub fn set_muted(&mut self, m: bool) {
        self.muted = m;
        if let Some(a) = &self.audio {
            a.set_muted(m);
        }
    }

    /// Folgt einer fremden Instanz: gleiche Position `t` (Sekunden) und gleicher Abspielzustand.
    /// Beim Abspielen wird nur bei Abweichung über `tol` (und erlaubter Korrektur) gesprungen;
    /// dabei wird die gemessene Seek-Dauer vorgehalten. Rückgabe: `true`, wenn ein Sprung ausgelöst wurde.
    pub fn follow(&mut self, t: f64, playing: bool, tol: f64, may_correct: bool) -> bool {
        let dur = self.duration();
        let t = if dur > 0.0 {
            t.clamp(0.0, dur)
        } else {
            t.max(0.0)
        };
        if playing {
            let off = if self.playing {
                (self.position() - t).abs()
            } else {
                f64::INFINITY
            };
            if self.playing && !(off > tol && may_correct) {
                return false;
            }
            let was_playing = self.playing;
            self.seek_time(t + self.seek_latency);
            if !was_playing {
                self.set_playing(true);
            }
            true
        } else {
            if self.playing {
                self.set_playing(false);
            }
            let Some(idx) = self.index.clone() else {
                return false;
            };
            let n = idx.frame_at(t);
            if self.frame_no() == Some(n) && self.target.is_none() {
                return false;
            }
            self.goto_frame(n);
            true
        }
    }

    pub fn toggle(&mut self) {
        if !self.playing && self.at_end() {
            // Am Ende: von vorn beginnen.
            let t0 = self.index.as_ref().and_then(|i| i.pts_of(0)).unwrap_or(0.0);
            self.hard_seek(t0, t0);
        }
        self.set_playing(!self.playing);
    }
}
