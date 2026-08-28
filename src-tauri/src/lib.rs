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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
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

            let hotkey = app.state::<AppState>().config.read().hotkey.clone();
            if let Err(e) = register_hotkey(&handle, &hotkey) {
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
        .run(|_app, event| {
            // Rnote persists every note on write, so there is nothing else to do
            // on exit — this arm exists only to retire the API service file.
            if let RunEvent::Exit = event {
                api::remove_service_file();
            }
        });
}
