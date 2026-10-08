//! Synchronisierte Wiedergabe mehrerer Instanzen (jede Instanz ist ein eigener Prozess).
//!
//! Die Instanzen tauschen kleine UDP-Nachrichten über `127.0.0.1` aus (keine Firewall-Abfrage,
//! keine Dateien, keine Registry). Jede Instanz bindet den ersten freien Port im Bereich
//! `PORT_BASE..PORT_BASE + PORT_COUNT` und sendet an alle anderen Ports des Bereichs.
//!
//! Protokoll (eine Textzeile pro Paket):
//! * `FS1 <id> hello <sync 0|1> <t> <playing 0|1>` – Lebenszeichen alle 2 s (auch ohne Sync)
//! * `FS1 <id> state <t> <playing 0|1> [<start_at_ms>]` – Zustand des Führenden: bei jeder Aktion und 2×/s
//!   beim Abspielen; `start_at_ms` kündigt einen gemeinsamen Start an (Unix-Millisekunden)
//!
//! `t` ist die Position auf der gemeinsamen Zeitachse (Sekunden). Jede Instanz hat einen
//! festen Versatz `offset` zwischen gemeinsamer und eigener Zeit (siehe `SyncController::align`).

use crate::player::Player;
use crossbeam_channel::{unbounded, Receiver};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PORT_BASE: u16 = 47600;
pub const PORT_COUNT: u16 = 64;

const HELLO_EVERY: Duration = Duration::from_secs(2);
const BEAT_EVERY: Duration = Duration::from_millis(500);
const PEER_TIMEOUT: Duration = Duration::from_secs(6);
/// Ab dieser Abweichung (Sekunden) springt ein Folgender beim Abspielen nach.
const DRIFT_TOLERANCE: f64 = 0.1;
/// Mindestabstand zwischen zwei Korrektur-Sprüngen.
const CORRECTION_COOLDOWN: Duration = Duration::from_millis(800);
/// Vorlauf, mit dem ein gemeinsamer Start angekündigt wird (reicht für Vorbereitung/Seek der Folgefenster).
const START_LEAD: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Hello {
        sync: bool,
        t: f64,
        playing: bool,
    },
    /// `start_at_ms`: angekündigter gemeinsamer Start (Unix-Millisekunden), sonst sofort.
    State {
        t: f64,
        playing: bool,
        start_at_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub from: u64,
    pub msg: Msg,
}

pub fn encode(from: u64, msg: &Msg) -> String {
    match msg {
        Msg::Hello { sync, t, playing } => {
            format!(
                "FS1 {from} hello {} {t:.6} {}",
                u8::from(*sync),
                u8::from(*playing)
            )
        }
        Msg::State {
            t,
            playing,
            start_at_ms,
        } => {
            let start = start_at_ms.map_or(String::new(), |ms| format!(" {ms}"));
            format!("FS1 {from} state {t:.6} {}{start}", u8::from(*playing))
        }
    }
}

pub fn decode(text: &str) -> Option<Packet> {
    let mut it = text.split_whitespace();
    if it.next()? != "FS1" {
        return None;
    }
    let from = it.next()?.parse().ok()?;
    let msg = match it.next()? {
        "hello" => {
            let sync = it.next()? == "1";
            let t: f64 = it.next()?.parse().ok()?;
            let playing = it.next()? == "1";
            Msg::Hello { sync, t, playing }
        }
        "state" => {
            let t: f64 = it.next()?.parse().ok()?;
            let playing = it.next()? == "1";
            let start_at_ms = it.next().and_then(|v| v.parse().ok());
            Msg::State {
                t,
                playing,
                start_at_ms,
            }
        }
        _ => return None,
    };
    t_is_finite(&msg).then_some(Packet { from, msg })
}

fn t_is_finite(msg: &Msg) -> bool {
    match msg {
        Msg::Hello { t, .. } | Msg::State { t, .. } => t.is_finite(),
    }
}

/// UDP-Anbindung: ein Empfangs-Thread, Senden direkt aus dem UI-Thread.
pub struct SyncNet {
    socket: Arc<UdpSocket>,
    own_port: u16,
    id: u64,
    rx: Receiver<Packet>,
}

impl SyncNet {
    /// Bindet den ersten freien Port; `None`, wenn alle belegt sind oder das Netzwerk fehlt.
    /// `wake` weckt die UI bei eingehenden Paketen.
    pub fn start(wake: impl Fn() + Send + 'static) -> Option<Self> {
        let (socket, own_port) = (PORT_BASE..PORT_BASE + PORT_COUNT).find_map(|port| {
            UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
                .ok()
                .map(|s| (s, port))
        })?;
        let socket = Arc::new(socket);
        let (tx, rx) = unbounded();
        let recv_socket = socket.clone();
        let spawned = std::thread::Builder::new()
            .name("sync-recv".into())
            .spawn(move || {
                let mut buf = [0u8; 256];
                loop {
                    match recv_socket.recv_from(&mut buf) {
                        Ok((n, _)) => {
                            let Ok(text) = std::str::from_utf8(&buf[..n]) else {
                                continue;
                            };
                            if let Some(packet) = decode(text) {
                                if tx.send(packet).is_err() {
                                    return;
                                }
                                wake();
                            }
                        }
                        // Windows meldet ICMP „Port nicht erreichbar“ früherer Sendungen als Fehler.
                        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            });
        spawned.ok()?;
        let id = u64::from(std::process::id()) << 32
            ^ u64::from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos()),
            );
        Some(Self {
            socket,
            own_port,
            id,
            rx,
        })
    }

    #[cfg(test)]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Sendet an alle anderen Ports des Bereichs (nicht vorhandene Empfänger sind harmlos).
    pub fn send(&self, msg: &Msg) {
        let text = encode(self.id, msg);
        for port in PORT_BASE..PORT_BASE + PORT_COUNT {
            if port != self.own_port {
                let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
                let _ = self.socket.send_to(text.as_bytes(), addr);
            }
        }
    }

    pub fn try_recv(&self) -> Option<Packet> {
        self.rx.try_recv().ok()
    }
}

#[derive(Debug, Clone)]
struct Peer {
    last_seen: Instant,
    sync: bool,
    /// Zuletzt gemeldete Position auf der gemeinsamen Zeitachse.
    t: f64,
    seen_at: Instant,
    playing: bool,
}

impl Peer {
    /// Extrapolierte Position (läuft die Wiedergabe, schreitet sie seit der Meldung fort).
    fn position_now(&self) -> f64 {
        if self.playing {
            self.t + self.seen_at.elapsed().as_secs_f64()
        } else {
            self.t
        }
    }
}

/// Steuert die Synchronisierung für eine Instanz.
pub struct SyncController {
    net: Option<SyncNet>,
    pub enabled: bool,
    /// Eigene Zeit = gemeinsame Zeit + `offset` (Sekunden).
    pub offset: f64,
    leader: bool,
    peers: HashMap<u64, Peer>,
    seen_events: u64,
    last_beat: Instant,
    last_hello: Instant,
    last_correction: Instant,
}

impl SyncController {
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let now = Instant::now();
        Self {
            net: SyncNet::start(wake),
            enabled: false,
            offset: 0.0,
            leader: false,
            peers: HashMap::new(),
            seen_events: 0,
            last_beat: now,
            last_hello: now.checked_sub(HELLO_EVERY).unwrap_or(now),
            last_correction: now.checked_sub(CORRECTION_COOLDOWN).unwrap_or(now),
        }
    }

    /// Ist das Netzwerk verfügbar (ein freier Port wurde gefunden)?
    pub fn available(&self) -> bool {
        self.net.is_some()
    }

    /// Anzahl anderer Instanzen mit aktiviertem Sync.
    pub fn synced_peers(&self) -> usize {
        self.peers.values().filter(|p| p.sync).count()
    }

    pub fn set_enabled(&mut self, on: bool, player: Option<&Player>) {
        if self.enabled == on {
            return;
        }
        self.enabled = on && self.net.is_some();
        self.leader = false;
        if let Some(p) = player {
            self.seen_events = p.events;
        }
    }

    /// Setzt den Versatz so, dass die eigene aktuelle Position der des zuletzt aktiven
    /// Sync-Partners entspricht. `false`, wenn kein Partner bekannt ist.
    pub fn align(&mut self, local_pos: f64) -> bool {
        let Some(peer) = self
            .peers
            .values()
            .filter(|p| p.sync)
            .max_by_key(|p| p.seen_at)
        else {
            return false;
        };
        self.offset = local_pos - peer.position_now();
        true
    }

    /// Je Frame aufrufen: Pakete verarbeiten, lokale Aktionen und Takt senden.
    pub fn update(&mut self, player: Option<&mut Player>) {
        let Some(net) = self.net.as_ref() else { return };
        let now = Instant::now();
        let mut incoming = Vec::new();
        while let Some(packet) = net.try_recv() {
            incoming.push(packet);
        }
        let mut player = player;

        for packet in incoming {
            match packet.msg {
                Msg::Hello { sync, t, playing } => {
                    self.peers.insert(
                        packet.from,
                        Peer {
                            last_seen: now,
                            sync,
                            t,
                            seen_at: now,
                            playing,
                        },
                    );
                }
                Msg::State {
                    t,
                    playing,
                    start_at_ms,
                } => {
                    let peer = self.peers.entry(packet.from).or_insert(Peer {
                        last_seen: now,
                        sync: true,
                        t,
                        seen_at: now,
                        playing,
                    });
                    *peer = Peer {
                        last_seen: now,
                        sync: true,
                        t,
                        seen_at: now,
                        playing,
                    };
                    crate::dlog::log(|| {
                        format!("recv state t={t:.3} playing={playing} start_at_ms={start_at_ms:?} enabled={}", self.enabled)
                    });
                    if self.enabled {
                        self.leader = false;
                        if let Some(p) = player.as_deref_mut() {
                            let start_at = start_at_ms.map(|ms| {
                                now + Duration::from_millis(ms.saturating_sub(unix_ms()))
                            });
                            self.follow(p, t + self.offset, playing, start_at, now);
                        }
                    }
                }
            }
        }
        self.peers
            .retain(|_, p| now.duration_since(p.last_seen) < PEER_TIMEOUT);

        let Some(player) = player else { return };
        let Some(net) = self.net.as_ref() else { return };
        if player.events < self.seen_events {
            self.seen_events = player.events; // neues Video geöffnet
        }
        // Läuft Sync mit Partnern, wird ein Start angekündigt, damit alle gemeinsam loslaufen.
        player.play_delay = (self.enabled && self.synced_peers() > 0).then_some(START_LEAD);
        // Pausiert liefert `position()` die Zielposition (auch während eines Seeks).
        let local_pos = player.position();
        let shared_t = local_pos - self.offset;

        // Lokale Bedienung (Play/Pause/Seek/Schritt/Loop-Sprung) → diese Instanz führt.
        if player.events != self.seen_events {
            self.seen_events = player.events;
            if self.enabled {
                self.leader = true;
                self.last_beat = now;
                crate::dlog::log(|| {
                    format!(
                        "lokale Aktion -> sende (t={shared_t:.3}, playing={})",
                        player.playing
                    )
                });
                let start_at_ms = player.scheduled_start().map(|at| {
                    unix_ms()
                        + u64::try_from(at.saturating_duration_since(now).as_millis()).unwrap_or(0)
                });
                net.send(&Msg::State {
                    t: shared_t,
                    playing: player.playing || start_at_ms.is_some(),
                    start_at_ms,
                });
            }
        }
        // Takt, solange der Führende abspielt.
        if self.enabled
            && self.leader
            && player.playing
            && now.duration_since(self.last_beat) >= BEAT_EVERY
        {
            self.last_beat = now;
            net.send(&Msg::State {
                t: shared_t,
                playing: true,
                start_at_ms: None,
            });
        }
        if now.duration_since(self.last_hello) >= HELLO_EVERY {
            self.last_hello = now;
            net.send(&Msg::Hello {
                sync: self.enabled,
                t: shared_t,
                playing: player.playing,
            });
        }
    }

    /// Folgt dem Zustand des Führenden.
    fn follow(
        &mut self,
        player: &mut Player,
        t: f64,
        playing: bool,
        start_at: Option<Instant>,
        now: Instant,
    ) {
        let cooldown_over = now.duration_since(self.last_correction) >= CORRECTION_COOLDOWN;
        if player.follow(t, playing, DRIFT_TOLERANCE, cooldown_over, start_at) {
            self.last_correction = now;
        }
        // Eigene Folge-Aktionen sind keine Bedienung durch den Nutzer.
        self.seen_events = player.events;
    }
}

/// Aktuelle Zeit als Unix-Millisekunden (alle Instanzen laufen auf demselben Rechner).
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_messages() {
        let msgs = [
            Msg::Hello {
                sync: true,
                t: 12.345678,
                playing: false,
            },
            Msg::State {
                t: 0.0,
                playing: true,
                start_at_ms: None,
            },
            Msg::State {
                t: 1234.5,
                playing: false,
                start_at_ms: None,
            },
            Msg::State {
                t: 5.25,
                playing: true,
                start_at_ms: Some(1_790_000_000_123),
            },
        ];
        for m in msgs {
            let packet = decode(&encode(42, &m)).expect("decode");
            assert_eq!(packet, Packet { from: 42, msg: m });
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("").is_none());
        assert!(decode("HELLO 1 state 1 1").is_none());
        assert!(decode("FS1 x state 1 1").is_none());
        assert!(decode("FS1 1 state nan 1").is_none());
        assert!(decode("FS1 1 state 1").is_none());
        assert!(decode("FS1 1 boom 1 1").is_none());
    }

    #[test]
    fn two_instances_exchange_messages() {
        let a = SyncNet::start(|| {}).expect("a");
        let b = SyncNet::start(|| {}).expect("b");
        assert_ne!(a.id(), b.id());
        a.send(&Msg::State {
            t: 3.5,
            playing: true,
            start_at_ms: Some(42),
        });
        let mut got = None;
        for _ in 0..100 {
            if let Some(p) = b.try_recv() {
                got = Some(p);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let p = got.expect("Paket empfangen");
        assert_eq!(p.from, a.id());
        assert_eq!(
            p.msg,
            Msg::State {
                t: 3.5,
                playing: true,
                start_at_ms: Some(42)
            }
        );
    }

    #[test]
    fn align_sets_offset_from_latest_peer() {
        let mut c = SyncController::new(|| {});
        assert!(!c.align(10.0), "ohne Partner kein Abgleich");
        let now = Instant::now();
        c.peers.insert(
            1,
            Peer {
                last_seen: now,
                sync: true,
                t: 4.0,
                seen_at: now,
                playing: false,
            },
        );
        assert!(c.align(10.0));
        // Eigene Zeit 10 s entspricht gemeinsamer Zeit 4 s → Versatz +6 s.
        assert!((c.offset - 6.0).abs() < 1e-9);
        // Partner ohne Sync zählen nicht.
        c.peers.get_mut(&1).expect("peer").sync = false;
        assert!(!c.align(10.0));
    }

    #[test]
    fn peer_position_extrapolates_only_when_playing() {
        let now = Instant::now();
        let paused = Peer {
            last_seen: now,
            sync: true,
            t: 5.0,
            seen_at: now,
            playing: false,
        };
        assert_eq!(paused.position_now(), 5.0);
        let playing = Peer {
            seen_at: now - Duration::from_secs(2),
            playing: true,
            ..paused
        };
        assert!(playing.position_now() >= 6.99);
    }
}
