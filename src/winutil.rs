//! Windows-spezifisch: Fenster aller laufenden FrameScope-Instanzen gleich groß, lückenlos
//! und ohne Überlappung in einem Raster anordnen.
//!
//! Jede Instanz ist ein eigener Prozess; die Fenster werden über die Win32-API gefunden
//! (sichtbares Hauptfenster, Titel endet auf „FrameScope“, Prozess = dieselbe EXE).

/// Anzahl Spalten und Zeilen für `n` Fenster: bis drei Fenster übereinander, danach ein
/// möglichst quadratisches Raster (4 → 2×2, 5–6 → 3×2, 7–9 → 3×3, …).
pub fn grid_dims(n: usize) -> (usize, usize) {
    match n {
        0 => (0, 0),
        1..=3 => (1, n),
        _ => {
            let cols = (1..).find(|c| c * c >= n).unwrap_or(1);
            (cols, n.div_ceil(cols))
        }
    }
}

/// Rechteck `(x, y, breite, höhe)` der Zelle `index` (zeilenweise) im Bereich
/// `(links, oben, breite, höhe)`. Die Kanten werden gerundet berechnet, damit zwischen
/// den Zellen keine Lücken entstehen und die letzte Spalte/Zeile den Rest übernimmt.
pub fn cell_rect(
    index: usize,
    cols: usize,
    rows: usize,
    area: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
    let (left, top, width, height) = area;
    let (col, row) = (index % cols, index / cols);
    let edge = |i: usize, total: i32, parts: usize| total * i as i32 / parts as i32;
    let (x0, x1) = (edge(col, width, cols), edge(col + 1, width, cols));
    let (y0, y1) = (edge(row, height, rows), edge(row + 1, height, rows));
    (left + x0, top + y0, x1 - x0, y1 - y0)
}

/// Ordnet alle FrameScope-Fenster auf dem Monitor des eigenen Fensters an.
/// Rückgabe: Anzahl der angeordneten Fenster.
#[cfg(windows)]
pub fn arrange_windows() -> Result<usize, String> {
    imp::arrange()
}

#[cfg(not(windows))]
pub fn arrange_windows() -> Result<usize, String> {
    Err("Anordnen wird nur unter Windows unterstützt".into())
}

#[cfg(windows)]
mod imp {
    use super::{cell_rect, grid_dims};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, IsZoomed, SetWindowPos, ShowWindow, GW_OWNER, SWP_NOACTIVATE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SW_RESTORE,
    };

    struct Found {
        hwnd: HWND,
        own: bool,
        top: i32,
        left: i32,
    }

    /// Dateiname (klein geschrieben) der EXE eines Prozesses.
    fn exe_name(pid: u32) -> Option<String> {
        // SAFETY: Handle wird nach der Abfrage geschlossen; Puffer ist ausreichend groß.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_lowercase)
        }
    }

    fn window_title(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: `buf` ist gültig und die Länge wird mitgegeben.
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
    }

    struct Ctx {
        own_exe: String,
        own_pid: u32,
        found: Vec<Found>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` ist der Zeiger auf das `Ctx`, das `arrange` für die Dauer von
        // `EnumWindows` am Leben hält.
        let ctx = unsafe { &mut *(lparam as *mut Ctx) };
        // SAFETY: einfache Abfragen auf ein von EnumWindows geliefertes Fenster.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
                return 1;
            }
            if !window_title(hwnd).ends_with("FrameScope") {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if exe_name(pid).as_deref() != Some(ctx.own_exe.as_str()) {
                return 1;
            }
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetWindowRect(hwnd, &mut rect);
            // Minimierte Fenster liegen weit außerhalb → ans Ende sortieren.
            let minimized = IsIconic(hwnd) != 0;
            ctx.found.push(Found {
                hwnd,
                own: pid == ctx.own_pid,
                top: if minimized { i32::MAX } else { rect.top },
                left: if minimized { i32::MAX } else { rect.left },
            });
        }
        1
    }

    pub fn arrange() -> Result<usize, String> {
        let own_exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
            .ok_or("EXE-Name unbekannt")?;
        // SAFETY: reine Abfrage.
        let own_pid = unsafe { GetCurrentProcessId() };
        let mut ctx = Ctx {
            own_exe,
            own_pid,
            found: Vec::new(),
        };
        // SAFETY: `ctx` lebt über den ganzen Aufruf; der Callback greift nur darauf zu.
        unsafe {
            EnumWindows(Some(collect), &mut ctx as *mut Ctx as LPARAM);
        }
        let own = ctx
            .found
            .iter()
            .find(|f| f.own)
            .ok_or("Eigenes Fenster nicht gefunden")?;

        // Arbeitsbereich (ohne Taskleiste) des Monitors, auf dem dieses Fenster liegt.
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: `info` ist initialisiert und `cbSize` gesetzt.
        let ok = unsafe {
            let monitor = MonitorFromWindow(own.hwnd, MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(monitor, &mut info)
        };
        if ok == 0 {
            return Err("Monitor-Informationen nicht verfügbar".into());
        }
        let area = (
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right - info.rcWork.left,
            info.rcWork.bottom - info.rcWork.top,
        );

        ctx.found.sort_by_key(|f| (f.top, f.left));
        let (cols, rows) = grid_dims(ctx.found.len());
        for (i, f) in ctx.found.iter().enumerate() {
            let (x, y, w, h) = cell_rect(i, cols, rows, area);
            // SAFETY: gültige Fenster-Handles aus EnumWindows; Fehler (z. B. beendetes Fenster) sind harmlos.
            unsafe {
                if IsIconic(f.hwnd) != 0 || IsZoomed(f.hwnd) != 0 {
                    ShowWindow(f.hwnd, SW_RESTORE);
                }
                SetWindowPos(
                    f.hwnd,
                    std::ptr::null_mut(),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        Ok(ctx.found.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_sizes() {
        assert_eq!(grid_dims(0), (0, 0));
        assert_eq!(grid_dims(1), (1, 1));
        assert_eq!(grid_dims(2), (1, 2)); // übereinander
        assert_eq!(grid_dims(3), (1, 3));
        assert_eq!(grid_dims(4), (2, 2));
        assert_eq!(grid_dims(5), (3, 2));
        assert_eq!(grid_dims(6), (3, 2));
        assert_eq!(grid_dims(9), (3, 3));
        assert_eq!(grid_dims(10), (4, 3));
        assert_eq!(grid_dims(16), (4, 4));
    }

    #[test]
    fn cells_cover_area_without_gaps() {
        let area = (100, 50, 1921, 1033); // bewusst ungerade Maße
        for n in 1..=12 {
            let (cols, rows) = grid_dims(n);
            let rects: Vec<_> = (0..n).map(|i| cell_rect(i, cols, rows, area)).collect();
            for (i, r) in rects.iter().enumerate() {
                assert!(r.2 > 0 && r.3 > 0, "n={n} i={i}");
                let col = i % cols;
                // Rechter Nachbar beginnt exakt am Ende der Zelle.
                if col + 1 < cols && i + 1 < n {
                    assert_eq!(r.0 + r.2, rects[i + 1].0, "n={n} i={i}");
                }
                // Untere Zelle beginnt exakt am Ende der Zelle.
                if i + cols < n {
                    assert_eq!(r.1 + r.3, rects[i + cols].1, "n={n} i={i}");
                }
            }
            // Erste Zelle beginnt links oben, die letzte Spalte/Zeile endet am Rand.
            assert_eq!((rects[0].0, rects[0].1), (area.0, area.1));
            let last_in_row = cols.min(n) - 1;
            assert_eq!(rects[last_in_row].0 + rects[last_in_row].2, area.0 + area.2);
        }
    }

    #[test]
    fn two_windows_stack_with_equal_height() {
        let (cols, rows) = grid_dims(2);
        let a = cell_rect(0, cols, rows, (0, 0, 1920, 1040));
        let b = cell_rect(1, cols, rows, (0, 0, 1920, 1040));
        assert_eq!((a.2, a.3), (1920, 520));
        assert_eq!((b.0, b.1, b.2, b.3), (0, 520, 1920, 520));
    }
}
