// dictation.rs
//
// System-wide dictation mode: speak anywhere, and transcribed text is typed
// into whatever window currently has keyboard focus (like Win+H voice typing,
// but fully local via the existing Whisper/Parakeet pipeline).
//
// Toggled by the Win+Shift+Z global shortcut (see lib.rs). Reuses the normal
// recording machinery with a mic-only session, so dictation sessions also show
// up in the meetings list as "Dictation <timestamp>" with their transcript.

use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static TOGGLE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn is_dictation_active() -> bool {
    DICTATION_ACTIVE.load(Ordering::SeqCst)
}

/// Toggle dictation from the global hotkey.
///
/// Deliberately does NOT focus the main window (unlike the tray/Win+Z toggle):
/// the transcribed text must land in the app that currently has keyboard focus.
pub fn toggle_dictation<R: Runtime>(app: &AppHandle<R>) {
    if TOGGLE_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        warn!("Dictation toggle already in progress, ignoring");
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if DICTATION_ACTIVE.load(Ordering::SeqCst) {
            stop_dictation(&app).await;
        } else if crate::is_recording().await {
            warn!("Dictation not started: a recording is already in progress");
            notify(
                &app,
                "Dictation unavailable",
                "A meeting recording is already in progress.",
            );
        } else {
            start_dictation(&app).await;
        }
        TOGGLE_IN_PROGRESS.store(false, Ordering::SeqCst);
    });
}

async fn start_dictation<R: Runtime>(app: &AppHandle<R>) {
    // Resolve microphone: preferred device from settings if still available,
    // otherwise the system default. System audio is intentionally excluded so
    // playback (music, videos) never gets typed as text.
    let preferred_mic = match crate::audio::recording_preferences::load_recording_preferences(app).await {
        Ok(prefs) => prefs.preferred_mic_device,
        Err(e) => {
            warn!("Failed to load recording preferences for dictation: {}", e);
            None
        }
    };

    let mic_name = match preferred_mic {
        Some(name) if crate::audio::parse_audio_device(&name).is_ok() => Some(name),
        _ => crate::audio::default_input_device().ok().map(|d| d.to_string()),
    };

    let Some(mic_name) = mic_name else {
        error!("Dictation start failed: no microphone available");
        notify(app, "Dictation failed", "No microphone device available.");
        return;
    };

    let meeting_name = format!(
        "Dictation {}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Starting dictation session '{}' with mic '{}'", meeting_name, mic_name);

    match crate::audio::recording_commands::start_recording_with_devices_and_meeting(
        app.clone(),
        Some(mic_name),
        None, // no system audio
        Some(meeting_name),
    )
    .await
    {
        Ok(()) => {
            DICTATION_ACTIVE.store(true, Ordering::SeqCst);
            show_indicator(app);
            notify(
                app,
                "Dictation started",
                "Speak — text will be typed into the focused window. Press Win+Shift+Z to stop.",
            );
        }
        Err(e) => {
            error!("Failed to start dictation: {}", e);
            notify(app, "Dictation failed", &e);
        }
    }
}

async fn stop_dictation<R: Runtime>(app: &AppHandle<R>) {
    info!("Stopping dictation session");

    // Same save-path convention as the tray toggle (stop_recording ignores it
    // for the actual file name but requires the argument).
    let save_path = match app.path().app_data_dir() {
        Ok(dir) => {
            let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
            dir.join(format!("recording-{}.wav", timestamp))
                .to_string_lossy()
                .to_string()
        }
        Err(e) => {
            error!("Failed to get app data dir for dictation stop: {}", e);
            String::new()
        }
    };

    let stop_result = crate::audio::recording_commands::stop_recording(
        app.clone(),
        crate::audio::recording_commands::RecordingArgs { save_path },
    )
    .await;

    // Cleared AFTER stop completes so the tail of speech (chunks still in the
    // transcription queue when the hotkey was pressed) still gets typed.
    DICTATION_ACTIVE.store(false, Ordering::SeqCst);
    hide_indicator(app);

    match stop_result {
        Ok(()) => {
            // Trigger frontend post-processing (SQLite save), same as tray toggle.
            if let Err(e) = app.emit("recording-stop-complete", true) {
                error!("Dictation: failed to emit recording-stop-complete: {}", e);
            }
            notify(app, "Dictation stopped", "Dictation session ended.");
        }
        Err(e) => {
            error!("Failed to stop dictation: {}", e);
            notify(app, "Dictation error", &format!("Failed to stop dictation: {}", e));
        }
    }
}

/// Type a transcribed segment into the currently focused window, followed by a
/// trailing space so consecutive segments don't run together.
pub fn type_transcribed_text(text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let with_space = format!("{} ", text);

    #[cfg(target_os = "windows")]
    send_unicode_text(&with_space);

    #[cfg(not(target_os = "windows"))]
    warn!(
        "Dictation typing not implemented on this platform; dropped text: {}",
        with_space
    );
}

/// Inject text as synthetic keyboard input via SendInput with KEYEVENTF_UNICODE.
/// Works for any Unicode text (each UTF-16 code unit is sent as a key event),
/// independent of the active keyboard layout. Note: cannot type into windows of
/// elevated (admin) processes unless Meetily itself runs elevated.
#[cfg(target_os = "windows")]
fn send_unicode_text(text: &str) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    let units: Vec<u16> = text.encode_utf16().collect();
    let mut inputs: Vec<INPUT> = Vec::with_capacity(units.len() * 2);

    for unit in units {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }

    if inputs.is_empty() {
        return;
    }

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };

    if sent as usize != inputs.len() {
        warn!(
            "Dictation SendInput injected {}/{} events (input may be blocked by the focused app)",
            sent,
            inputs.len()
        );
    }
}

const INDICATOR_LABEL: &str = "dictation-indicator";
const INDICATOR_WIDTH: f64 = 120.0;
const INDICATOR_HEIGHT: f64 = 40.0;

/// Show a tiny always-on-top "Dictating" pill in the bottom-right corner.
/// The window is click-through and never takes focus, so it cannot interfere
/// with where the typed text lands.
fn show_indicator<R: Runtime>(app: &AppHandle<R>) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    // Reuse a leftover window if one exists (e.g. after an aborted session).
    if let Some(win) = app.get_webview_window(INDICATOR_LABEL) {
        let _ = win.show();
        return;
    }

    let win = match WebviewWindowBuilder::new(
        app,
        INDICATOR_LABEL,
        WebviewUrl::App("dictation-indicator.html".into()),
    )
    .title("Dictation")
    .inner_size(INDICATOR_WIDTH, INDICATOR_HEIGHT)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .shadow(false)
    .build()
    {
        Ok(win) => win,
        Err(e) => {
            warn!("Failed to create dictation indicator window: {}", e);
            return;
        }
    };

    // Click-through: the pill is purely informational.
    if let Err(e) = win.set_ignore_cursor_events(true) {
        warn!("Failed to make dictation indicator click-through: {}", e);
    }

    // Bottom-right corner of the primary monitor, above a typical taskbar.
    match win.primary_monitor() {
        Ok(Some(monitor)) => {
            let scale = monitor.scale_factor();
            let size = monitor.size();
            let pos = monitor.position();
            let w = (INDICATOR_WIDTH * scale) as i32;
            let h = (INDICATOR_HEIGHT * scale) as i32;
            let margin = (16.0 * scale) as i32;
            let taskbar_clearance = (48.0 * scale) as i32;
            let x = pos.x + size.width as i32 - w - margin;
            let y = pos.y + size.height as i32 - h - margin - taskbar_clearance;
            if let Err(e) = win.set_position(tauri::PhysicalPosition::new(x, y)) {
                warn!("Failed to position dictation indicator: {}", e);
            }
        }
        _ => warn!("Could not determine primary monitor for dictation indicator"),
    }
}

fn hide_indicator<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window(INDICATOR_LABEL) {
        if let Err(e) = win.close() {
            warn!("Failed to close dictation indicator: {}", e);
        }
    }
}

fn notify<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        warn!("Failed to show dictation notification '{}': {}", title, e);
    }
}
