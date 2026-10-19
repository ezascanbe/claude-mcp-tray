#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod restart;

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

/// When the popup last lost focus. Clicking the tray icon steals focus first,
/// so without this a click meant to close the popup would immediately reopen it.
struct LastHidden(Mutex<Option<Instant>>);

/// When the popup was last shown; Windows can report a spurious blur right after showing.
struct ShownAt(Mutex<Option<Instant>>);

/// Last known tray icon rectangle in physical pixels: (x, y, width, height).
struct TrayRect(Mutex<Option<(f64, f64, f64, f64)>>);

#[tauri::command]
fn list_servers() -> Result<config::Listing, String> {
    config::list().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn set_server_enabled(name: String, enabled: bool) -> Result<config::Listing, String> {
    config::set_enabled(&name, enabled).map_err(|e| format!("{e:#}"))?;
    list_servers()
}

#[tauri::command]
fn set_extension_enabled(id: String, enabled: bool) -> Result<config::Listing, String> {
    config::set_extension_enabled(&id, enabled).map_err(|e| format!("{e:#}"))?;
    list_servers()
}

#[tauri::command]
async fn restart_claude() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(restart::restart_claude)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn open_config_folder() -> Result<(), String> {
    let path = config::config_path();
    let dir = path.parent().ok_or("config path has no parent")?;
    open_path(dir).map_err(|e| e.to_string())
}

fn open_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let cmd = "explorer.exe";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd).arg(path).spawn().map(|_| ())
}

/// Whether the taskbar / panel the tray icon sits on uses a light theme.
#[cfg(windows)]
fn taskbar_is_light() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|v| v != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn taskbar_is_light() -> bool {
    false // macOS uses a template image the system recolours; Linux panels are usually dark
}

fn tray_icon(light_taskbar: bool) -> tauri::image::Image<'static> {
    let bytes: &'static [u8] = if cfg!(target_os = "macos") {
        include_bytes!("../icons/tray-template.png")
    } else if light_taskbar {
        include_bytes!("../icons/tray-black.png")
    } else {
        include_bytes!("../icons/tray-white.png")
    };
    tauri::image::Image::from_bytes(bytes).expect("embedded tray icon is a valid PNG")
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Places the popup next to the tray icon, on whichever screen edge the
/// taskbar / menu bar occupies, clamped inside the monitor's work area.
fn position_popup(app: &AppHandle, win: &WebviewWindow) {
    const MARGIN: f64 = 8.0;
    let tray = *app.state::<TrayRect>().0.lock().unwrap();
    let monitor = tray
        .and_then(|(x, y, w, h)| app.monitor_from_point(x + w / 2.0, y + h / 2.0).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, win.outer_size()) else { return };

    let area = monitor.work_area();
    let (left, top) = (area.position.x as f64, area.position.y as f64);
    let (right, bottom) = (left + area.size.width as f64, top + area.size.height as f64);
    let (w, h) = (size.width as f64, size.height as f64);

    // Default: bottom-right corner of the work area.
    let (mut x, mut y) = (right - w - MARGIN, bottom - h - MARGIN);
    if let Some((tx, ty, tw, th)) = tray {
        let (cx, cy) = (tx + tw / 2.0, ty + th / 2.0);
        if ty >= bottom - 1.0 {
            (x, y) = (cx - w / 2.0, bottom - h - MARGIN); // taskbar at bottom
        } else if ty + th <= top + 1.0 {
            (x, y) = (cx - w / 2.0, top + MARGIN); // menu bar / taskbar at top
        } else if tx + tw <= left + 1.0 {
            (x, y) = (left + MARGIN, cy - h / 2.0); // taskbar on the left
        } else if tx >= right - 1.0 {
            (x, y) = (right - w - MARGIN, cy - h / 2.0); // taskbar on the right
        } else {
            (x, y) = (cx - w / 2.0, ty - h - MARGIN); // tray inside work area (overflow flyout)
        }
    }
    x = x.clamp(left + MARGIN, (right - w - MARGIN).max(left));
    y = y.clamp(top + MARGIN, (bottom - h - MARGIN).max(top));
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn show_popup(app: &AppHandle) {
    let Some(win) = main_window(app) else { return };
    position_popup(app, &win);
    *app.state::<ShownAt>().0.lock().unwrap() = Some(Instant::now());
    let _ = win.emit("refresh", ());
    let _ = win.show();
    let _ = win.set_focus();
}

fn toggle_popup(app: &AppHandle) {
    let Some(win) = main_window(app) else { return };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    let recently_hidden = app
        .state::<LastHidden>()
        .0
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !recently_hidden {
        show_popup(app);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_popup(app)))
        .manage(LastHidden(Mutex::new(None)))
        .manage(ShownAt(Mutex::new(None)))
        .manage(TrayRect(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            list_servers,
            set_server_enabled,
            set_extension_enabled,
            restart_claude,
            open_config_folder
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let open = MenuItem::with_id(app, "open", "Show MCP servers & extensions", true, None::<&str>)?;
            let folder = MenuItem::with_id(app, "folder", "Open config folder", true, None::<&str>)?;
            let logs = MenuItem::with_id(app, "logs", "View logs", config::logs_dir().is_some(), None::<&str>)?;
            let restart = MenuItem::with_id(app, "restart", "Restart Claude", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &folder, &logs, &restart, &sep, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(tray_icon(taskbar_is_light()))
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("Claude MCP servers & extensions")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_popup(app),
                    "folder" => {
                        let _ = open_config_folder();
                    }
                    "logs" => {
                        if let Some(dir) = config::logs_dir() {
                            let _ = open_path(&dir);
                        }
                    }
                    "restart" => {
                        std::thread::spawn(restart::restart_claude);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let scale = app
                            .get_webview_window("main")
                            .and_then(|w| w.scale_factor().ok())
                            .unwrap_or(1.0);
                        let pos = rect.position.to_physical::<f64>(scale);
                        let size = rect.size.to_physical::<f64>(scale);
                        *app.state::<TrayRect>().0.lock().unwrap() =
                            Some((pos.x, pos.y, size.width, size.height));
                        toggle_popup(app);
                    }
                })
                .build(app)?;

            // Keep the tray glyph legible when the user switches the taskbar between light and dark.
            #[cfg(windows)]
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let mut light = taskbar_is_light();
                    loop {
                        std::thread::sleep(Duration::from_secs(3));
                        let now = taskbar_is_light();
                        if now != light {
                            light = now;
                            if let Some(tray) = handle.tray_by_id("main") {
                                let _ = tray.set_icon(Some(tray_icon(light)));
                            }
                        }
                    }
                });
            }

            if std::env::var_os("CLAUDE_MCP_TRAY_SHOW").is_some() {
                show_popup(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                // Debug aid: keep the popup open for screenshots.
                if cfg!(debug_assertions) && std::env::var_os("CLAUDE_MCP_TRAY_SHOW").is_some() {
                    return;
                }
                let just_shown = window
                    .state::<ShownAt>()
                    .0
                    .lock()
                    .unwrap()
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                if just_shown {
                    let _ = window.set_focus();
                    return;
                }
                *window.state::<LastHidden>().0.lock().unwrap() = Some(Instant::now());
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running claude-mcp-tray");
}
