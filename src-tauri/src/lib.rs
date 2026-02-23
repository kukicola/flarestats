mod commands;

use std::sync::Mutex;
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt};

struct TrayRect(Mutex<Option<(PhysicalPosition<f64>, PhysicalSize<f64>)>>);

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(FlareStatsPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })

    panel_event!(FlareStatsPanelEventHandler {
        window_did_resign_key(notification: &NSNotification) -> ()
    })
}

#[cfg(target_os = "macos")]
fn init_panel(app: &tauri::AppHandle) {
    let window = app.get_webview_window("main").unwrap();
    let panel = window.to_panel::<FlareStatsPanel>().unwrap();

    panel.set_has_shadow(false);
    panel.set_opaque(false);
    panel.set_level(PanelLevel::MainMenu.value() + 1);
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .move_to_active_space()
            .full_screen_auxiliary()
            .value(),
    );
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().value());

    let event_handler = FlareStatsPanelEventHandler::new();
    let handle = app.clone();
    event_handler.window_did_resign_key(move |_notification| {
        if let Ok(panel) = handle.get_webview_panel("main") {
            panel.hide();
        }
    });
    panel.set_event_handler(Some(event_handler.as_ref()));
}

/// Position the panel below the tray icon and show it.
#[cfg(target_os = "macos")]
fn show_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Some((pos, size)) = *app.state::<TrayRect>().0.lock().unwrap() {
            let scale = window.scale_factor().unwrap_or(1.0);
            let panel_w = window.outer_size().map(|s| s.width as f64).unwrap_or(420.0 * scale);
            let x = pos.x + size.width / 2.0 - panel_w / 2.0;
            let y = pos.y + size.height;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
    if let Ok(panel) = app.get_webview_panel("main") {
        panel.show_and_make_key();
    }
}

fn store_tray_rect(app: &tauri::AppHandle, event: &TrayIconEvent) {
    let rect = match event {
        TrayIconEvent::Click { rect, .. }
        | TrayIconEvent::Enter { rect, .. }
        | TrayIconEvent::Move { rect, .. } => rect,
        _ => return,
    };
    *app.state::<TrayRect>().0.lock().unwrap() = Some((
        rect.position.to_physical(1.0),
        rect.size.to_physical(1.0),
    ));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .manage(TrayRect(Mutex::new(None)))
        .manage(commands::RefreshTask(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::fetch_analytics,
            commands::start_background_refresh,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &settings, &separator, &quit])?;

            let tray_icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;

            TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .tooltip("FlareStats")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => show_panel(app),
                        "settings" => {
                            show_panel(app);
                            let _ = app.emit("open-settings", ());
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    let app = tray.app_handle();
                    store_tray_rect(app, &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Ok(panel) = app.get_webview_panel("main") {
                            if panel.is_visible() { panel.hide(); } else { show_panel(app); }
                        }
                    }
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            init_panel(app.handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
