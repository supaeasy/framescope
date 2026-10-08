//! Timecode-Formatierung (`hh:mm:ss:ff`, nicht drop-frame).
//!
//! `ff` ist die Position des Frames innerhalb seiner Sekunde, gezählt anhand der
//! tatsächlichen Präsentationszeiten (siehe `FrameIndex::frame_in_second`). Bei
//! konstanter Framerate entspricht das dem üblichen Timecode, bei VFR ist es ein
//! laufender Zähler der in dieser Sekunde dargestellten Frames.

/// Formatiert Zeit `t` (Sekunden) mit der gegebenen Frame-Position innerhalb der Sekunde.
pub fn format(t: f64, ff: u32) -> String {
    let total = (t.max(0.0) + 1e-4).floor() as u64;
    let (s, m_total) = (total % 60, total / 60);
    let (m, h) = (m_total % 60, m_total / 60);
    format!("{h:02}:{m:02}:{s:02}:{ff:02}")
}

/// Näherung ohne Index: `ff` aus Nachkommastellen und nomineller Framerate.
pub fn format_nominal(t: f64, fps: f64) -> String {
    let t = t.max(0.0);
    let ff = ((t - (t + 1e-4).floor()).max(0.0) * fps).round() as u32;
    format(t, ff.min(fps.ceil() as u32 - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_formatting() {
        assert_eq!(format(0.0, 0), "00:00:00:00");
        assert_eq!(format(61.0, 5), "00:01:01:05");
        assert_eq!(format(3661.5, 12), "01:01:01:12");
    }

    #[test]
    fn rounding_just_below_second_boundary() {
        // 0.99999999 s ist numerisch die volle Sekunde.
        assert_eq!(format(0.999_999_99, 0), "00:00:01:00");
    }

    #[test]
    fn nominal_fallback() {
        assert_eq!(format_nominal(1.5, 30.0), "00:00:01:15");
        assert_eq!(format_nominal(2.0, 25.0), "00:00:02:00");
        assert_eq!(format_nominal(0.96, 25.0), "00:00:00:24");
    }
}
