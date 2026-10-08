//! Audioausgabe und Master-Clock.
//!
//! Ein eigener Thread dekodiert den Audiostream (FFmpeg), konvertiert per swresample
//! nach Stereo/f32 in der Samplerate des Ausgabegeräts und liefert Chunks über einen
//! Channel an den cpal-Callback. Der Callback zählt die tatsächlich ausgegebenen
//! Samples und stellt daraus die Wiedergabezeit bereit (Audio = Master-Clock).

use anyhow::{anyhow, Context as _, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender, TryRecvError};
use ffmpeg::software::resampling;
use ffmpeg::{format, format::sample::Type as SampleType, format::Sample, media::Type};
use ffmpeg::{util::frame::audio::Audio as AudioFrame, ChannelLayout};
use ffmpeg_next as ffmpeg;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const AV_TIME_BASE: f64 = 1_000_000.0;
/// Anzahl vorgepufferter Chunks (je ca. ein Audioframe, 20–40 ms).
const CHUNK_QUEUE: usize = 24;

struct Chunk {
    serial: u64,
    /// Zeit des ersten Samples in Sekunden (Videozeitbasis).
    pts: f64,
    /// Interleaved Stereo, f32.
    data: Vec<f32>,
}

enum Cmd {
    Seek { serial: u64, time: f64 },
}

/// Zwischen UI, Callback und Decoder geteilter Zustand.
struct Shared {
    volume: AtomicU32,
    muted: AtomicBool,
    playing: AtomicBool,
    serial: AtomicU64,
    /// Ist `clock_*` für die aktuelle Seek-Generation gültig?
    valid: AtomicBool,
    /// Wiedergabezeit (f64-Bits) zum Zeitpunkt `stamp_ns`.
    clock_bits: AtomicU64,
    stamp_ns: AtomicU64,
    epoch: Instant,
}

pub struct AudioEngine {
    shared: Arc<Shared>,
    cmd: Sender<Cmd>,
    // Der Stream muss leben, solange Ton ausgegeben wird (nicht Send → bleibt im UI-Thread).
    _stream: cpal::Stream,
}

impl AudioEngine {
    /// Startet die Audioausgabe. `None`, wenn kein Audiostream oder Gerät vorhanden ist.
    pub fn start(path: &Path, volume: f32, muted: bool) -> Option<Self> {
        has_audio_stream(path).then_some(())?;
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        let rate = config.sample_rate();
        let channels = usize::from(config.channels());
        let format = config.sample_format();

        let shared = Arc::new(Shared {
            volume: AtomicU32::new(volume.to_bits()),
            muted: AtomicBool::new(muted),
            playing: AtomicBool::new(false),
            serial: AtomicU64::new(0),
            valid: AtomicBool::new(false),
            clock_bits: AtomicU64::new(0f64.to_bits()),
            stamp_ns: AtomicU64::new(0),
            epoch: Instant::now(),
        });
        let (chunk_tx, chunk_rx) = bounded(CHUNK_QUEUE);
        let (cmd_tx, cmd_rx) = unbounded();

        let thread_path = path.to_path_buf();
        thread::Builder::new()
            .name("audio-decoder".into())
            .spawn(move || {
                // Fehler (z. B. exotischer Codec) → Video läuft ohne Ton weiter.
                let _ = decode_loop(&thread_path, rate, &cmd_rx, &chunk_tx);
            })
            .ok()?;

        let mixer = Mixer {
            rx: chunk_rx,
            shared: shared.clone(),
            cur: None,
            pos: 0,
            channels,
            rate,
        };
        let stream_config = config.config();
        let stream = match format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, stream_config, mixer),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, stream_config, mixer),
            _ => return None,
        }
        .ok()?;
        stream.play().ok()?;
        Some(Self {
            shared,
            cmd: cmd_tx,
            _stream: stream,
        })
    }

    pub fn set_volume(&self, v: f32) {
        self.shared
            .volume
            .store(v.clamp(0.0, 1.0).to_bits(), Relaxed);
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Relaxed);
    }

    /// Ausgabe starten/anhalten (Puffer bleiben erhalten).
    pub fn set_playing(&self, playing: bool) {
        self.shared.playing.store(playing, Relaxed);
    }

    /// Verwirft gepufferten Ton und beginnt bei `time` (Sekunden).
    pub fn seek(&self, serial: u64, time: f64) {
        self.shared.valid.store(false, Relaxed);
        self.shared.serial.store(serial, Relaxed);
        let _ = self.cmd.send(Cmd::Seek { serial, time });
    }

    /// Aktuelle Wiedergabezeit laut Audioausgabe, sobald sie für die aktuelle
    /// Seek-Generation gültig ist.
    pub fn clock(&self) -> Option<f64> {
        if !self.shared.valid.load(Relaxed) || !self.shared.playing.load(Relaxed) {
            return None;
        }
        let base = f64::from_bits(self.shared.clock_bits.load(Relaxed));
        let stamp = Duration::from_nanos(self.shared.stamp_ns.load(Relaxed));
        Some(
            base + self
                .shared
                .epoch
                .elapsed()
                .saturating_sub(stamp)
                .as_secs_f64(),
        )
    }
}

fn has_audio_stream(path: &Path) -> bool {
    format::input(path).is_ok_and(|i| i.streams().best(Type::Audio).is_some())
}

/// Wandelt den internen f32-Stereo-Strom in das Gerätesampleformat.
trait OutSample: cpal::SizedSample + Send + 'static {
    fn from_f32(v: f32) -> Self;
}
impl OutSample for f32 {
    fn from_f32(v: f32) -> Self {
        v
    }
}
impl OutSample for i16 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
    }
}

fn build_stream<T: OutSample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut mixer: Mixer,
) -> Result<cpal::Stream> {
    device
        .build_output_stream(
            config,
            move |out: &mut [T], info: &cpal::OutputCallbackInfo| mixer.fill(out, info),
            |_err| {},
            None,
        )
        .map_err(|e| anyhow!("Audioausgabe nicht verfügbar: {e}"))
}

/// Läuft im Audio-Callback: holt Chunks, wendet Lautstärke an, führt die Clock.
struct Mixer {
    rx: Receiver<Chunk>,
    shared: Arc<Shared>,
    cur: Option<Chunk>,
    /// Bereits ausgegebene Frames (Stereo-Paare) des aktuellen Chunks.
    pos: usize,
    channels: usize,
    rate: u32,
}

impl Mixer {
    fn fill<T: OutSample>(&mut self, out: &mut [T], info: &cpal::OutputCallbackInfo) {
        let silence = T::from_f32(0.0);
        out.fill(silence);
        if !self.shared.playing.load(Relaxed) || self.channels == 0 {
            return;
        }
        let gain = if self.shared.muted.load(Relaxed) {
            0.0
        } else {
            let v = f32::from_bits(self.shared.volume.load(Relaxed));
            v * v // quadratische Kennlinie: feinere Regelung bei leisen Pegeln
        };
        let serial = self.shared.serial.load(Relaxed);
        let mut produced = false;
        for frame in out.chunks_mut(self.channels) {
            // Passenden Chunk besorgen (veraltete verwerfen).
            while self
                .cur
                .as_ref()
                .is_none_or(|c| c.serial != serial || self.pos * 2 >= c.data.len())
            {
                match self.rx.try_recv() {
                    Ok(c) => {
                        self.cur = Some(c);
                        self.pos = 0;
                    }
                    Err(_) => {
                        self.cur = None;
                        break;
                    }
                }
            }
            let Some(c) = &self.cur else { break };
            let (l, r) = (c.data[self.pos * 2] * gain, c.data[self.pos * 2 + 1] * gain);
            self.pos += 1;
            produced = true;
            match frame {
                [m] => *m = T::from_f32((l + r) * 0.5),
                [a, b, ..] => {
                    *a = T::from_f32(l);
                    *b = T::from_f32(r);
                }
                [] => {}
            }
        }
        if produced {
            if let Some(c) = &self.cur {
                // Gerätelatenz abziehen: Samples sind erst `latency` später hörbar.
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(ts.callback).as_secs_f64();
                let now = c.pts + self.pos as f64 / f64::from(self.rate) - latency;
                self.shared.clock_bits.store(now.to_bits(), Relaxed);
                let stamp =
                    u64::try_from(self.shared.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                self.shared.stamp_ns.store(stamp, Relaxed);
                self.shared.valid.store(c.serial == serial, Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Decoder-Thread
// ---------------------------------------------------------------------------

struct AudioSource {
    input: format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::decoder::Audio,
    time_base: f64,
    /// Startzeit des Videostreams (gemeinsamer Nullpunkt mit der Videozeit).
    start: f64,
    rate: u32,
    resampler: Option<(resampling::Context, Sample, ChannelLayout, u32)>,
    eof_sent: bool,
    next_pts: f64,
    skip_until: f64,
}

fn ratio(r: ffmpeg::Rational) -> f64 {
    f64::from(r.numerator()) / f64::from(r.denominator().max(1))
}

impl AudioSource {
    fn open(path: &Path, rate: u32) -> Result<Self> {
        let input = format::input(path).context("Audio: Datei nicht lesbar")?;
        let stream = input
            .streams()
            .best(Type::Audio)
            .ok_or_else(|| anyhow!("kein Audiostream"))?;
        let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = ctx
            .decoder()
            .audio()
            .context("Audiocodec wird nicht unterstützt")?;
        let start = input
            .streams()
            .best(Type::Video)
            .map(|v| v.start_time())
            .filter(|&t| t != ffmpeg::ffi::AV_NOPTS_VALUE)
            .map_or(0.0, |t| {
                let tb = input
                    .streams()
                    .best(Type::Video)
                    .map_or(0.0, |v| ratio(v.time_base()));
                t as f64 * tb
            });
        Ok(Self {
            stream_index: stream.index(),
            time_base: ratio(stream.time_base()),
            input,
            decoder,
            start,
            rate,
            resampler: None,
            eof_sent: false,
            next_pts: 0.0,
            skip_until: f64::NEG_INFINITY,
        })
    }

    fn seek(&mut self, time: f64) -> Result<()> {
        let ts = ((time.max(0.0) + self.start) * AV_TIME_BASE) as i64;
        self.input
            .seek(ts, ..ts)
            .context("Audio-Seek fehlgeschlagen")?;
        self.decoder.flush();
        self.resampler = None; // verwirft internen Resampler-Puffer
        self.eof_sent = false;
        self.skip_until = time;
        Ok(())
    }

    /// Nächster Chunk (Stereo/f32 in Gerätesamplerate); `None` = Stream-Ende.
    fn next_chunk(&mut self) -> Result<Option<(f64, Vec<f32>)>> {
        let mut frame = AudioFrame::empty();
        loop {
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    if let Some(chunk) = self.convert(&frame)? {
                        return Ok(Some(chunk));
                    }
                    continue;
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                Err(ffmpeg::Error::InvalidData) => continue,
                Err(e) => return Err(e).context("Audio-Dekodierfehler"),
            }
            if self.eof_sent {
                return Ok(None);
            }
            match self.input.packets().next() {
                Some((s, p)) if s.index() == self.stream_index => {
                    match self.decoder.send_packet(&p) {
                        Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
                        Err(e) => return Err(e).context("Audio-Dekodierfehler"),
                    }
                }
                Some(_) => {}
                None => {
                    self.decoder.send_eof().ok();
                    self.eof_sent = true;
                }
            }
        }
    }

    /// Konvertiert einen dekodierten Frame; schneidet Samples vor `skip_until` ab.
    fn convert(&mut self, frame: &AudioFrame) -> Result<Option<(f64, Vec<f32>)>> {
        let pts = match frame.pts() {
            Some(ts) => ts as f64 * self.time_base - self.start,
            None => self.next_pts,
        };
        self.next_pts = pts + frame.samples() as f64 / f64::from(frame.rate().max(1));

        let layout = if frame.channel_layout().is_empty() {
            ChannelLayout::default(i32::from(frame.channels().max(1)))
        } else {
            frame.channel_layout()
        };
        let (fmt, in_rate) = (frame.format(), frame.rate());
        let reusable = matches!(&self.resampler, Some((_, f, l, r)) if *f == fmt && *l == layout && *r == in_rate);
        if !reusable {
            let ctx = resampling::Context::get(
                fmt,
                layout,
                in_rate,
                Sample::F32(SampleType::Packed),
                ChannelLayout::STEREO,
                self.rate,
            )
            .context("Resampler konnte nicht erstellt werden")?;
            self.resampler = Some((ctx, fmt, layout, in_rate));
        }
        let (resampler, ..) = self
            .resampler
            .as_mut()
            .ok_or_else(|| anyhow!("Resampler fehlt"))?;
        // Ausgabepuffer selbst dimensionieren: `run` alloziert sonst nur `input.samples()`,
        // was beim Hochrechnen der Samplerate Samples im Resampler zurückhält.
        let ratio = f64::from(self.rate) / f64::from(in_rate.max(1));
        let queued = resampler
            .delay()
            .map_or(0, |d| usize::try_from(d.output).unwrap_or(0));
        let capacity = (frame.samples() as f64 * ratio).ceil() as usize + queued + 256;
        let mut out = AudioFrame::new(
            Sample::F32(SampleType::Packed),
            capacity,
            ChannelLayout::STEREO,
        );
        resampler
            .run(frame, &mut out)
            .context("Resampling fehlgeschlagen")?;
        let n = out.samples();
        if n == 0 {
            return Ok(None);
        }
        let bytes = out.data(0);
        let mut data: Vec<f32> = bytes
            .get(..n * 2 * 4)
            .ok_or_else(|| anyhow!("Ungültiger Audiopuffer"))?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();

        let mut pts = pts;
        let rate = f64::from(self.rate);
        if pts < self.skip_until {
            let drop_frames = ((self.skip_until - pts) * rate).round() as usize;
            if drop_frames * 2 >= data.len() {
                return Ok(None);
            }
            data.drain(..drop_frames * 2);
            pts = self.skip_until;
        }
        Ok(Some((pts, data)))
    }
}

fn decode_loop(path: &Path, rate: u32, cmd_rx: &Receiver<Cmd>, tx: &Sender<Chunk>) -> Result<()> {
    let mut src = AudioSource::open(path, rate)?;
    let mut serial = 0u64;
    loop {
        // Nur das jeweils letzte Seek zählt.
        let mut latest = None;
        loop {
            match cmd_rx.try_recv() {
                Ok(c) => latest = Some(c),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some(Cmd::Seek { serial: s, time }) = latest {
            src.seek(time)?;
            serial = s;
        }

        let Some((pts, data)) = src.next_chunk()? else {
            // Ende: auf ein Seek warten.
            match cmd_rx.recv() {
                Ok(Cmd::Seek { serial: s, time }) => {
                    src.seek(time)?;
                    serial = s;
                    continue;
                }
                Err(_) => return Ok(()),
            }
        };
        let chunk = Chunk { serial, pts, data };
        select! {
            send(tx, chunk) -> r => { if r.is_err() { return Ok(()); } }
            recv(cmd_rx) -> c => match c {
                Ok(Cmd::Seek { serial: s, time }) => { src.seek(time)?; serial = s; }
                Err(_) => return Ok(()),
            },
        }
    }
}

/// Entwickler-Selbsttest (`framescope --audio-selftest <datei>`): spielt stumm 3 s ab und
/// gibt aus, wie weit die Audio-Clock gegenüber der Systemuhr abweicht.
pub fn selftest(path: &Path) -> Result<()> {
    let engine =
        AudioEngine::start(path, 0.0, true).ok_or_else(|| anyhow!("kein Audio verfügbar"))?;
    engine.seek(1, 4.0);
    engine.set_playing(true);
    let start = Instant::now();
    let mut first: Option<(f64, f64)> = None;
    while start.elapsed() < Duration::from_secs(4) {
        if let Some(c) = engine.clock() {
            let wall = start.elapsed().as_secs_f64();
            let (c0, w0) = *first.get_or_insert((c, wall));
            println!(
                "wall {wall:6.3}  clock {c:6.3}  Δ(clock-4.0-wall) {:+.3}  Δ seit Start {:+.3}",
                c - 4.0 - wall,
                (c - c0) - (wall - w0)
            );
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}
