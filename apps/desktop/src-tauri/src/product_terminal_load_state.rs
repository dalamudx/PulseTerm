//! Read-only load checks; compiled and registered only in the explicit probe build.
use super::*;
#[path = "output_timing_probe.rs"]
mod output_timing;
use output_timing::output_timings;

#[cfg(target_os = "linux")]
pub(super) struct NativeTiming {
    records: Mutex<output_timing::NativeTimings>,
}
#[cfg(target_os = "linux")]
impl NativeTiming {
    pub(super) fn new(generation: u64) -> Option<Arc<Self>> {
        (std::env::var("PULSE_ECHO_TIMING").as_deref() == Ok("1")).then(|| {
            Arc::new(Self {
                records: Mutex::new(output_timing::NativeTimings::new(generation)),
            })
        })
    }
    pub(super) fn now() -> u64 {
        let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        u64::try_from(time.tv_sec)
            .ok()
            .and_then(|sec| sec.checked_mul(1_000_000_000))
            .and_then(|sec| {
                u64::try_from(time.tv_nsec)
                    .ok()
                    .and_then(|ns| sec.checked_add(ns))
            })
            .unwrap_or(0)
    }
    pub(super) fn sending(&self, bytes: &[u8]) -> Option<u32> {
        let now = Self::now();
        self.records.lock().ok()?.sending(bytes, now)
    }
    pub(super) fn sent(&self, sequence: Option<u32>, success: bool) {
        let now = Self::now();
        if let Ok(mut records) = self.records.lock() {
            records.sent(sequence, now, success);
        }
    }
    pub(super) fn acknowledged(&self, sequence: u32, now: u64) {
        if let Ok(mut records) = self.records.lock() {
            records.acknowledged(sequence, now);
        }
    }
    fn export(&self, browser: &output_timing::OutputTimings) -> Result<()> {
        let report = self
            .records
            .lock()
            .map_err(|_| "workspace.denied")?
            .snapshot(browser)?;
        let text = serde_json::to_string(&report).map_err(|_| "workspace.denied")?;
        eprintln!("workspace.echo_native_timing:{text}");
        Ok(())
    }
}
fn drawing(
    value: &CaptureDrawing,
    source_is_utf8: bool,
) -> Result<pulse_app::registry::RenderedScreen> {
    fn number(value: &str) -> Result<u64> {
        let parsed = value.parse::<u64>().map_err(|_| "workspace.denied")?;
        if parsed.to_string() != value {
            return Err("workspace.denied");
        }
        Ok(parsed)
    }
    Ok(pulse_app::registry::RenderedScreen {
        boundary: pulse_term::session::SessionState {
            lifecycle: pulse_term::session::Lifecycle::Running,
            size: pulse_core::TermSize::new(value.columns, value.rows)
                .map_err(|_| "workspace.denied")?,
            parsed_bytes: number(&value.source_bytes)?,
            utf8_bytes: number(&value.utf8_bytes)?,
            source_is_utf8,
            revision: number(&value.revision)?,
        },
        digest: value.digest.clone(),
    })
}

#[tauri::command]
pub(in crate::product_workspace) async fn workspace_terminal_load_marker(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let entry = state.0.terminals.entry(nonce, ticket(&connection)?)?;
    if !entry.confirmed.load(std::sync::atomic::Ordering::SeqCst)
        || !entry
            .input_displayed
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("workspace.denied");
    }
    entry
        .binding
        .request_marker()
        .await
        .map_err(|_| "workspace.denied")?;
    state.0.terminals.entry(nonce, ticket(&connection)?)?;
    eprintln!("workspace.load_marker_admitted");
    Ok(())
}

#[tauri::command]
pub(in crate::product_workspace) async fn workspace_terminal_load_compare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    rendered: CaptureDrawing,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let entry = state.0.terminals.entry(nonce, ticket(&connection)?)?;
    if !entry.confirmed.load(std::sync::atomic::Ordering::SeqCst)
        || !entry
            .input_displayed
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("workspace.denied");
    }
    let image = drawing(&rendered, entry.capture.source_is_utf8)?;
    let result = entry
        .binding
        .compare_visible(image)
        .await
        .map_err(|_| "workspace.denied")?;
    state.0.terminals.entry(nonce, ticket(&connection)?)?;
    match result {
        pulse_app::registry::VisibleComparison::Equal => {
            eprintln!(
                "workspace.load_screen_equal:{}:{}:{}:{}:{}:{}",
                rendered.columns,
                rendered.rows,
                rendered.revision,
                rendered.source_bytes,
                rendered.utf8_bytes,
                rendered.digest
            );
            Ok(())
        }
        pulse_app::registry::VisibleComparison::Mismatch => Err("workspace.load_screen_mismatch"),
        pulse_app::registry::VisibleComparison::Stale => Err("workspace.load_screen_stale"),
    }
}

/// Bounded numeric diagnostics only; no I/O, input or session authorization is granted.
#[tauri::command]
pub(in crate::product_workspace) async fn workspace_terminal_load_timings(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    connection: String,
    timing: String,
) -> Result<()> {
    let (nonce, _) = page(&window, &state, &nonce)?;
    let entry = state.0.terminals.entry(nonce, ticket(&connection)?)?;
    if !entry.confirmed.load(std::sync::atomic::Ordering::SeqCst)
        || !entry
            .input_displayed
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("workspace.denied");
    }
    let report = output_timings(&timing)?;
    #[cfg(target_os = "linux")]
    if let Some(timing) = &entry.native_timing {
        timing.export(&report)?;
    }
    let text = serde_json::to_string(&report).map_err(|_| "workspace.denied")?;
    eprintln!("workspace.echo_timing:{text}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> CaptureDrawing {
        CaptureDrawing {
            columns: 80,
            rows: 24,
            revision: "10".into(),
            source_bytes: "1073741824".into(),
            utf8_bytes: "1073741824".into(),
            digest: "0123456789abcdef".into(),
        }
    }
    #[test]
    fn exact_metadata_keeps_both_coordinates_and_fixed_running_lifecycle()
    -> std::result::Result<(), &'static str> {
        let mut value = sample();
        value.utf8_bytes = "1073741825".into();
        let parsed = drawing(&value, false)?;
        assert_eq!(parsed.boundary.parsed_bytes, 1073741824);
        assert_eq!(parsed.boundary.utf8_bytes, 1073741825);
        assert_eq!(parsed.boundary.revision, 10);
        assert!(!parsed.boundary.source_is_utf8);
        assert_eq!(
            parsed.boundary.lifecycle,
            pulse_term::session::Lifecycle::Running
        );
        assert_eq!(parsed.digest, value.digest);
        Ok(())
    }
    #[test]
    fn noncanonical_overflow_and_invalid_geometry_are_rejected() {
        for invalid in ["", "01", "+1", "-1", "1 ", "18446744073709551616"] {
            for coordinate in 0..3 {
                let mut value = sample();
                match coordinate {
                    0 => value.revision = invalid.into(),
                    1 => value.source_bytes = invalid.into(),
                    _ => value.utf8_bytes = invalid.into(),
                }
                assert!(drawing(&value, true).is_err());
            }
        }
        let mut value = sample();
        value.columns = 0;
        assert!(drawing(&value, true).is_err());
    }
}
