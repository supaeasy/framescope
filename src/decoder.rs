//! Decoder-Thread: öffnet die Datei, dekodiert Frames (swscale → RGBA) und
//! liefert sie über einen Channel an die UI. Seeks werden per Command gesendet.

use anyhow::{anyhow, Context as _, Result};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender, TryRecvError};
use ffmpeg::{format, format::Pixel, media::Type, software::scaling, util::frame::video::Video};
use ffmpeg_next as ffmpeg;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

const AV_TIME_BASE: f64 = 1_000_000.0;
/// Anzahl vorab dekodierter Frames, die in der UI-Queue warten dürfen.
const QUEUE_DEPTH: usize = 3;

#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Dauer in Sekunden (0, falls unbekannt).
    pub duration: f64,
    pub codec: String,
}

/// Ein dekodierter Frame in Originalauflösung (RGBA, dicht gepackt).
pub struct Frame {
    /// Seek-Generation, zu der der Frame gehört (veraltete werden verworfen).
    pub serial: u64,
    /// Präsentationszeit in Sekunden (relativ zum Streamstart).
    pub pts: f64,
    pub key: bool,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub enum Command {
    Seek { serial: u64, time: f64 },
}

pub enum Event {
    Opened(VideoInfo),
    Frame(Arc<Frame>),
    /// Ende des Streams erreicht (für die angegebene Seek-Generation).
    Eof(u64),
    Error(String),
}

pub struct DecoderHandle {
    pub cmd: Sender<Command>,
    pub events: Receiver<Event>,
}

impl DecoderHandle {
    /// Startet den Decoder-Thread. `wake` weckt die UI (Repaint) bei neuen Events.
    /// Das Droppen des Handles beendet den Thread.
    pub fn spawn(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (cmd_tx, cmd_rx) = unbounded();
        let (ev_tx, ev_rx) = bounded(QUEUE_DEPTH);
        let spawned = thread::Builder::new()
            .name("decoder".into())
            .spawn(move || {
                if let Err(e) = run(&path, &cmd_rx, &ev_tx, &wake) {
                    let _ = ev_tx.send(Event::Error(format!("{e:#}")));
                    wake();
                }
            });
        if spawned.is_err() {
            eprintln!("Decoder-Thread konnte nicht gestartet werden");
        }
        Self {
            cmd: cmd_tx,
            events: ev_rx,
        }
    }
}

struct Source {
    input: format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    time_base: f64,
    start: f64,
    fps: f64,
    scaler: Option<(scaling::Context, Pixel, u32, u32)>,
    decoded: Video,
    rgba: Video,
    eof_sent: bool,
    last_pts: f64,
}

fn ratio(r: ffmpeg::Rational) -> f64 {
    f64::from(r.numerator()) / f64::from(r.denominator().max(1))
}

impl Source {
    fn open(path: &Path) -> Result<(Self, VideoInfo)> {
        let input = format::input(path)
            .with_context(|| format!("Datei kann nicht geöffnet werden: {}", path.display()))?;
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow!("Die Datei enthält keinen Videostream"))?;
        let stream_index = stream.index();
        let time_base = ratio(stream.time_base());
        let start = match stream.start_time() {
            t if t == ffmpeg::ffi::AV_NOPTS_VALUE => 0.0,
            t => t as f64 * time_base,
        };
        let mut fps = ratio(stream.avg_frame_rate());
        if !(fps.is_finite() && fps > 0.0) {
            fps = ratio(stream.rate());
        }
        if !(fps.is_finite() && fps > 0.0) {
            fps = 25.0;
        }
        let mut ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .context("Codec-Parameter ungültig")?;
        ctx.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });
        let decoder = ctx
            .decoder()
            .video()
            .context("Videocodec wird nicht unterstützt")?;
        let (width, height) = (decoder.width(), decoder.height());
        if width == 0 || height == 0 {
            return Err(anyhow!("Ungültige Videoauflösung"));
        }
        let duration = if input.duration() > 0 {
            input.duration() as f64 / AV_TIME_BASE
        } else {
            0.0
        };
        let info = VideoInfo {
            width,
            height,
            fps,
            duration,
            codec: decoder.id().name().to_string(),
        };
        let src = Self {
            input,
            stream_index,
            decoder,
            time_base,
            start,
            fps,
            scaler: None,
            decoded: Video::empty(),
            rgba: Video::empty(),
            eof_sent: false,
            last_pts: 0.0,
        };
        Ok((src, info))
    }

    fn seek(&mut self, time: f64) -> Result<()> {
        let ts = ((time.max(0.0) + self.start) * AV_TIME_BASE) as i64;
        self.input.seek(ts, ..ts).context("Seek fehlgeschlagen")?;
        self.decoder.flush();
        self.eof_sent = false;
        Ok(())
    }

    /// Dekodiert den nächsten Frame nach `self.decoded`. `None` = Stream-Ende.
    fn next_frame(&mut self) -> Result<Option<(f64, bool)>> {
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {
                    let pts = match self.decoded.timestamp() {
                        Some(ts) => ts as f64 * self.time_base - self.start,
                        None => self.last_pts + 1.0 / self.fps,
                    };
                    self.last_pts = pts;
                    return Ok(Some((pts, self.decoded.is_key())));
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                // Einzelne defekte Pakete überspringen statt abzubrechen.
                Err(ffmpeg::Error::InvalidData) => continue,
                Err(e) => return Err(e).context("Dekodierfehler"),
            }
            if self.eof_sent {
                return Ok(None);
            }
            match self.input.packets().next() {
                Some((stream, packet)) => {
                    if stream.index() == self.stream_index {
                        match self.decoder.send_packet(&packet) {
                            Ok(()) | Err(ffmpeg::Error::InvalidData) => {}
                            Err(e) => return Err(e).context("Dekodierfehler"),
                        }
                    }
                }
                None => {
                    self.decoder.send_eof().ok();
                    self.eof_sent = true;
                }
            }
        }
    }

    /// Konvertiert den zuletzt dekodierten Frame nach RGBA.
    fn convert_rgba(&mut self) -> Result<(usize, usize, Vec<u8>)> {
        let (w, h, fmt) = (
            self.decoded.width(),
            self.decoded.height(),
            self.decoded.format(),
        );
        let reusable =
            matches!(&self.scaler, Some((_, f, sw, sh)) if *f == fmt && *sw == w && *sh == h);
        if !reusable {
            let ctx = scaling::Context::get(fmt, w, h, Pixel::RGBA, w, h, scaling::Flags::BILINEAR)
                .context("Skalierer konnte nicht erstellt werden")?;
            self.scaler = Some((ctx, fmt, w, h));
            self.rgba = Video::new(Pixel::RGBA, w, h);
        }
        let (scaler, ..) = self
            .scaler
            .as_mut()
            .ok_or_else(|| anyhow!("Skalierer fehlt"))?;
        scaler
            .run(&self.decoded, &mut self.rgba)
            .context("Farbkonvertierung fehlgeschlagen")?;

        let (w, h) = (w as usize, h as usize);
        let stride = self.rgba.stride(0);
        let data = self.rgba.data(0);
        let mut out = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            let start = row * stride;
            let line = data
                .get(start..start + w * 4)
                .ok_or_else(|| anyhow!("Ungültiger Frame-Puffer"))?;
            out.extend_from_slice(line);
        }
        Ok((w, h, out))
    }
}

/// Seek-Zustand des Decoder-Threads.
struct SeekState {
    serial: u64,
    /// Frames vor dieser Zeit werden nach einem Seek verworfen.
    skip_until: f64,
    tolerance: f64,
}

impl SeekState {
    fn apply(&mut self, src: &mut Source, cmd: Command) -> Result<()> {
        let Command::Seek { serial, time } = cmd;
        src.seek(time)?;
        self.serial = serial;
        self.skip_until = time - self.tolerance;
        Ok(())
    }
}

fn run(
    path: &Path,
    cmd_rx: &Receiver<Command>,
    ev_tx: &Sender<Event>,
    wake: &impl Fn(),
) -> Result<()> {
    let (mut src, info) = Source::open(path)?;
    let mut st = SeekState {
        serial: 0,
        skip_until: f64::NEG_INFINITY,
        tolerance: 0.5 / info.fps,
    };
    if ev_tx.send(Event::Opened(info)).is_err() {
        return Ok(());
    }
    wake();

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
        if let Some(c) = latest {
            st.apply(&mut src, c)?;
        }

        let Some((pts, key)) = src.next_frame()? else {
            if ev_tx.send(Event::Eof(st.serial)).is_err() {
                return Ok(());
            }
            wake();
            match cmd_rx.recv() {
                Ok(c) => st.apply(&mut src, c)?,
                Err(_) => return Ok(()),
            }
            continue;
        };
        if pts < st.skip_until {
            continue;
        }
        let (width, height, rgba) = src.convert_rgba()?;
        let frame = Event::Frame(Arc::new(Frame {
            serial: st.serial,
            pts,
            key,
            width,
            height,
            rgba,
        }));
        // Senden, aber auf Commands (Seek) reagieren, falls die Queue voll ist.
        select! {
            send(ev_tx, frame) -> r => {
                if r.is_err() { return Ok(()); }
                wake();
            }
            recv(cmd_rx) -> c => match c {
                Ok(c) => st.apply(&mut src, c)?,
                Err(_) => return Ok(()),
            },
        }
    }
}
