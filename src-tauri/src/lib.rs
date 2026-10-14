pub mod api;
pub mod commands;
pub mod config;
pub mod notes;
pub mod state;
pub mod watcher;

use crate::config::Settings;
use crate::state::AppState;
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Emitted when the window is summoned, so the UI can focus the compose box.
const FOCUS_COMPOSE: &str = "focus-compose";

/// Emitted right before every `hide()`, so the UI gets one last chance to
/// flush an unsaved draft to disk. Hiding does not tear down the webview —
/// its JS keeps running in the background — so the async save this triggers
/// completes fine even though the window is no longer visible by the time it
/// resolves.
const WINDOW_HIDING: &str = "window-hiding";

/// Passed by the autostart entry so a login does not throw a window in your
/// face. Any launch without it is someone asking for the window.
pub const MINIMIZED_FLAG: &str = "--minimized";

/// The single path every hide goes through — the hotkey/tray toggle,
/// hide-on-blur, the close button, and the frontend's own `hide_window`
/// command all call this rather than `window.hide()` directly, so the
/// `WINDOW_HIDING` notice can never be skipped.
pub fn hide_window(app: &AppHandle) {
    let _ = app.emit(WINDOW_HIDING, ());
    save_window_geometry(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// Records where the window is, so the next launch puts it back there.
/// Called on every hide (which covers hotkey, blur, close, Esc) and on exit.
fn save_window_geometry(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    if size.width == 0 || size.height == 0 {
        return;
    }
    let geometry = config::WindowGeometry { x: pos.x, y: pos.y, width: size.width, height: size.height };
    if let Err(e) = config::save_window_geometry(&geometry) {
        eprintln!("[window] could not save geometry: {e}");
    }
}

/// Puts the window back where it was last time — unless that spot is no
/// longer on any monitor (a display was unplugged, resolution changed), in
/// which case the default centred placement stays, so it can't open
/// somewhere invisible.
fn restore_window_geometry(app: &AppHandle) {
    let (Some(window), Some(g)) = (app.get_webview_window("main"), config::load_window_geometry())
    else {
        return;
    };
    // Require a reasonable chunk of the window (its top strip, where it's
    // grabbed from) to land on some monitor.
    const MIN_VISIBLE: i32 = 80;
    let visible = window.available_monitors().unwrap_or_default().iter().any(|m| {
        let (mp, ms) = (m.position(), m.size());
        let overlap_x = (g.x + g.width as i32).min(mp.x + ms.width as i32) - g.x.max(mp.x);
        let top_on_screen = g.y >= mp.y && g.y + 20 <= mp.y + ms.height as i32;
        overlap_x >= MIN_VISIBLE && top_on_screen
    });
    if !visible {
        return;
    }
    let _ = window.set_size(tauri::PhysicalSize::new(g.width, g.height));
    let _ = window.set_position(tauri::PhysicalPosition::new(g.x, g.y));
}

/// Applies a UI zoom level, clamped to the supported range.
pub(crate) fn apply_zoom(app: &AppHandle, zoom: f64) -> f64 {
    let zoom = ((zoom.clamp(config::ZOOM_MIN, config::ZOOM_MAX)) * 100.0).round() / 100.0;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_zoom(zoom);
    }
    zoom
}

pub fn toggle_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let focused = app.state::<AppState>().focused.load(Ordering::Relaxed);
    if visible && focused {
        hide_window(app);
    } else {
        show_window(app);
    }
}

pub fn show_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit(FOCUS_COMPOSE, ());
}

/// Makes the Windows sign-in entry (`HKCU\...\Run`, written by the autostart
/// plugin with `--minimized`) match the setting. Enabling is redone on every
/// boot rather than only when missing, so the entry follows the exe if the
/// app was reinstalled somewhere else.
///
/// Skipped in debug builds: `npm run tauri dev` would otherwise register the
/// throwaway `target\debug` binary to start at every sign-in.
pub(crate) fn sync_autostart(app: &AppHandle, wanted: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if wanted {
        launcher.enable().map_err(|e| format!("could not enable autostart: {e}"))
    } else if launcher.is_enabled().unwrap_or(false) {
        launcher.disable().map_err(|e| format!("could not disable autostart: {e}"))
    } else {
        Ok(())
    }
}

pub(crate) fn register_hotkey(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| format!("{accelerator:?} is not a valid shortcut: {e:?}"))?;

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    manager
        .on_shortcut(shortcut, |app, _shortcut, event| {
            // The handler fires on press *and* release; acting on both would
            // toggle twice per keystroke and leave the window where it started.
            if event.state() == ShortcutState::Pressed {
                toggle_window(app);
            }
        })
        .map_err(|e| format!("could not register {accelerator:?}: {e}"))
}

/// Track focus by asking the OS which window is in front.
///
/// `WindowEvent::Focused` cannot carry this: with WebView2 the keyboard focus
/// lives in a child HWND, so the event fires only sometimes and
/// `Window::is_focused` answers `false` for a window you are typing into.
/// Hide-on-blur depends on knowing the truth here, so this polls for it
/// instead. One syscall every 200 ms.
#[cfg(windows)]
fn spawn_focus_watch(app: AppHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(handle) = window.hwnd() else {
        eprintln!("[focus] no window handle; hide-on-blur disabled");
        return;
    };
    let ours = handle.0 as isize;
    app.state::<AppState>().our_hwnd.store(ours, Ordering::SeqCst);

    std::thread::spawn(move || {
        let mut was_focused = true;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            // SAFETY: no arguments; returns the foreground HWND.
            let foreground = unsafe { GetForegroundWindow() } as isize;
            let focused = foreground == ours;
            if focused == was_focused {
                continue;
            }
            was_focused = focused;

            let state = app.state::<AppState>();
            state.focused.store(focused, Ordering::Relaxed);

            // Suppressed in debug builds, where opening devtools would
            // otherwise hide the window being inspected.
            if !focused && !cfg!(debug_assertions) && state.config.read().hide_on_blur {
                hide_window(&app);
            }
        }
    });
}

#[cfg(not(windows))]
fn spawn_focus_watch(_app: AppHandle) {}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show Rnote", true, None::<&str>)?,
            &MenuItem::with_id(app, "open_folder", "Open notes folder", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Rnote")
        // Left click should summon the window, not pop the menu.
        .show_menu_on_left_click(false)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "open_folder" => {
                if let Err(e) = commands::open_notes_folder(app.clone()) {
                    eprintln!("[tray] could not open notes folder: {e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg: Settings = config::load();
    let notes = notes::scan::scan_all(&cfg);
    eprintln!("[boot] loaded {} notes from {:?}", notes.len(), cfg.notes_dir);

    tauri::Builder::default()
        // Must come first: this plugin only wins the race for the lock if it
        // is registered before anything else can start doing work.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(cfg, notes))
        .invoke_handler(tauri::generate_handler![
            commands::list_notes,
            commands::list_tags,
            commands::rename_tag,
            commands::set_zoom,
            commands::get_note,
            commands::create_note,
            commands::update_note,
            commands::set_status,
            commands::delete_note,
            commands::open_notes_folder,
            commands::get_settings,
            commands::set_settings,
            commands::hide_window,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let (hotkey, autostart, zoom) = {
                let state = app.state::<AppState>();
                let cfg = state.config.read();
                (cfg.hotkey.clone(), cfg.autostart, cfg.zoom)
            };
            restore_window_geometry(&handle);
            if zoom != 1.0 {
                apply_zoom(&handle, zoom);
            }
            if let Err(e) = register_hotkey(&handle, &hotkey) {
                eprintln!("[boot] {e}");
            }
            if let Err(e) = sync_autostart(&handle, autostart) {
                eprintln!("[boot] {e}");
            }
            build_tray(&handle)?;

            if let Some(window) = app.get_webview_window("main") {
                let handle = handle.clone();
                window.on_window_event(move |event| {
                    // Closing must not tear the process down: the tray icon,
                    // hotkey, and watcher all need the process to stay alive.
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_window(&handle);
                    }
                });
            }

            spawn_focus_watch(handle.clone());
            watcher::spawn(handle.clone());
            api::spawn(handle.clone());
            if !std::env::args().any(|arg| arg == MINIMIZED_FLAG) {
                show_window(&handle);
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Rnote")
        .run(|app, event| match event {
            // Notes are persisted on every write; on the way out only the
            // window's spot (if it's showing) and the API registry entry
            // need handling.
            RunEvent::ExitRequested { .. } => save_window_geometry(app),
            RunEvent::Exit => api::remove_service_file(),
            _ => {}
        });
}
