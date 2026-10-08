//! Frame-Index: Präsentationszeiten und Keyframe-Flags aller Videopakete.
//!
//! Wird im Hintergrund durch reines Lesen der Pakete (ohne Dekodierung) aufgebaut.
//! Frame *n* ist der n-te Eintrag in Präsentationsreihenfolge (0-basiert); das ist
//! auch bei variabler Framerate (VFR) exakt, da jede Paket-PTS genau ein Frame ist.

use anyhow::{anyhow, Context as _, Result};
use ffmpeg_next as ffmpeg;
use std::path::Path;

/// Toleranz beim Vergleich von Zeitstempeln (Sekunden).
const EPS: f64 = 1e-6;

#[derive(Debug, Default)]
pub struct FrameIndex {
    /// Präsentationszeit je Frame in Sekunden (aufsteigend, relativ zum Streamstart).
    pts: Vec<f64>,
    /// Frame-Nummern der Keyframes (aufsteigend).
    keys: Vec<usize>,
}

impl FrameIndex {
    /// Baut den Index aus `(pts, ist_keyframe)`-Paaren in beliebiger Reihenfolge.
    pub fn from_packets(mut packets: Vec<(f64, bool)>) -> Self {
        packets.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut pts: Vec<f64> = Vec::with_capacity(packets.len());
        let mut keys = Vec::new();
        for (t, key) in packets {
            // Doppelte Zeitstempel (defekte Streams) zu einem Frame zusammenfassen.
            if pts.last().is_some_and(|&last| (t - last).abs() < EPS) {
                if key && keys.last() != Some(&(pts.len() - 1)) {
                    keys.push(pts.len() - 1);
                }
                continue;
            }
            if key {
                keys.push(pts.len());
            }
            pts.push(t);
        }
        Self { pts, keys }
    }

    /// Anzahl der Frames.
    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    pub fn pts_of(&self, frame: usize) -> Option<f64> {
        self.pts.get(frame).copied()
    }

    /// Letzter Frame, dessen Zeit ≤ `t` ist (0, falls `t` davor liegt).
    pub fn frame_at(&self, t: f64) -> usize {
        self.pts
            .partition_point(|&p| p <= t + EPS)
            .saturating_sub(1)
    }

    pub fn is_key(&self, frame: usize) -> bool {
        self.keys.binary_search(&frame).is_ok()
    }

    /// Letzter Keyframe vor `frame`.
    pub fn prev_key(&self, frame: usize) -> Option<usize> {
        let i = self.keys.partition_point(|&k| k < frame);
        i.checked_sub(1).map(|i| self.keys[i])
    }

    /// Erster Keyframe nach `frame`.
    pub fn next_key(&self, frame: usize) -> Option<usize> {
        let i = self.keys.partition_point(|&k| k <= frame);
        self.keys.get(i).copied()
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Zeitstempel aller Keyframes (für die Timeline-Marker).
    pub fn key_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.keys.iter().map(|&k| self.pts[k])
    }

    /// Zeit des letzten Frames.
    pub fn last_pts(&self) -> f64 {
        self.pts.last().copied().unwrap_or(0.0)
    }

    /// Position des Frames innerhalb seiner Sekunde (für den Timecode `hh:mm:ss:ff`).
    pub fn frame_in_second(&self, frame: usize) -> u32 {
        let Some(t) = self.pts_of(frame) else {
            return 0;
        };
        let second = (t + 1e-4).floor();
        let first = self.pts.partition_point(|&p| p < second - 1e-4);
        u32::try_from(frame.saturating_sub(first)).unwrap_or(0)
    }
}

/// Liest alle Videopakete der Datei und baut den Index (kein Decoding).
pub fn scan(path: &Path) -> Result<FrameIndex> {
    let mut input = ffmpeg::format::input(path).context("Index: Datei nicht lesbar")?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| anyhow!("Index: kein Videostream"))?;
    let index = stream.index();
    let tb = stream.time_base();
    let time_base = f64::from(tb.numerator()) / f64::from(tb.denominator().max(1));
    let start = match stream.start_time() {
        t if t == ffmpeg::ffi::AV_NOPTS_VALUE => 0.0,
        t => t as f64 * time_base,
    };
    let mut packets = Vec::with_capacity(4096);
    for (s, p) in input.packets() {
        if s.index() != index {
            continue;
        }
        if let Some(ts) = p.pts().or_else(|| p.dts()) {
            packets.push((ts as f64 * time_base - start, p.is_key()));
        }
    }
    Ok(FrameIndex::from_packets(packets))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Konstante Framerate; Pakete in Dekodierreihenfolge mit B-Frame-Umsortierung.
    fn cfr(fps: f64, n: usize, gop: usize) -> FrameIndex {
        let mut p: Vec<(f64, bool)> = (0..n).map(|i| (i as f64 / fps, i % gop == 0)).collect();
        p.swap(1, 2); // Reihenfolge darf egal sein
        FrameIndex::from_packets(p)
    }

    #[test]
    fn counts_and_keys() {
        let idx = cfr(25.0, 100, 25);
        assert_eq!(idx.len(), 100);
        assert_eq!(idx.key_count(), 4);
        assert!(idx.is_key(0) && idx.is_key(50) && !idx.is_key(51));
        assert_eq!(idx.prev_key(50), Some(25));
        assert_eq!(idx.prev_key(0), None);
        assert_eq!(idx.next_key(50), Some(75));
        assert_eq!(idx.next_key(75), None);
    }

    #[test]
    fn frame_at_time() {
        let idx = cfr(25.0, 100, 25);
        assert_eq!(idx.frame_at(-1.0), 0);
        assert_eq!(idx.frame_at(0.0), 0);
        assert_eq!(idx.frame_at(0.039), 0);
        assert_eq!(idx.frame_at(0.04), 1);
        assert_eq!(idx.frame_at(idx.pts_of(37).unwrap()), 37);
        assert_eq!(idx.frame_at(1000.0), 99);
    }

    #[test]
    fn frame_in_second_cfr() {
        let idx = cfr(25.0, 100, 25);
        assert_eq!(idx.frame_in_second(0), 0);
        assert_eq!(idx.frame_in_second(24), 24);
        assert_eq!(idx.frame_in_second(25), 0);
        assert_eq!(idx.frame_in_second(99), 24);
    }

    #[test]
    fn frame_in_second_ntsc() {
        let idx = cfr(30000.0 / 1001.0, 300, 30);
        // 29.97 fps: Frame 30 liegt bei 1.001 s → erste Position der 2. Sekunde.
        assert_eq!(idx.frame_in_second(29), 29);
        assert_eq!(idx.frame_in_second(30), 0);
        assert_eq!(idx.frame_in_second(299), 29);
    }

    #[test]
    fn variable_frame_rate() {
        // Frames bei 0, 0.5, 0.6, 1.0, 2.5 s → exakt 5 Frames, laufender Zähler je Sekunde.
        let idx = FrameIndex::from_packets(vec![
            (0.0, true),
            (0.5, false),
            (0.6, false),
            (1.0, true),
            (2.5, false),
        ]);
        assert_eq!(idx.len(), 5);
        assert_eq!(idx.frame_in_second(2), 2);
        assert_eq!(idx.frame_in_second(3), 0);
        assert_eq!(idx.frame_in_second(4), 0);
        assert_eq!(idx.frame_at(0.55), 1);
        assert_eq!(idx.frame_at(2.4), 3);
    }

    #[test]
    fn duplicate_timestamps_merge() {
        let idx = FrameIndex::from_packets(vec![(0.0, false), (0.0, true), (0.04, false)]);
        assert_eq!(idx.len(), 2);
        assert!(idx.is_key(0));
    }

    #[test]
    fn empty_index() {
        let idx = FrameIndex::default();
        assert!(idx.is_empty());
        assert_eq!(idx.frame_at(5.0), 0);
        assert_eq!(idx.frame_in_second(0), 0);
    }
}
