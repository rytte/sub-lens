use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager,
};
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;

const EVENT_STATE_CHANGED: &str = "overlay://state";
const STORE_PATH: &str = "overlay-settings.json";
const STORE_KEY: &str = "overlay";
const TRAY_ID: &str = "overlay-tray";
const DEFAULT_WIDTH: f64 = 720.0;
const DEFAULT_HEIGHT: f64 = 132.0;
const MIN_WIDTH: f64 = 280.0;
const MIN_HEIGHT: f64 = 72.0;

const MENU_TOGGLE_VISIBLE: &str = "toggle_visible";
const MENU_TOGGLE_LOCK: &str = "toggle_lock";
const MENU_EDIT_MODE: &str = "edit_mode";
const MENU_TOGGLE_AUTO_CENTER: &str = "toggle_auto_center";
const MENU_OPACITY_20: &str = "opacity_20";
const MENU_OPACITY_40: &str = "opacity_40";
const MENU_OPACITY_60: &str = "opacity_60";
const MENU_OPACITY_80: &str = "opacity_80";
const MENU_OPACITY_100: &str = "opacity_100";
const MENU_APPEARANCE_GLASS: &str = "appearance_glass";
const MENU_APPEARANCE_SOLID: &str = "appearance_solid";
const MENU_COLOR_BLACK: &str = "color_black";
const MENU_COLOR_CHARCOAL: &str = "color_charcoal";
const MENU_COLOR_SILVER: &str = "color_silver";
const MENU_COLOR_WHITE: &str = "color_white";
const MENU_RESET_POSITION: &str = "reset_position";
const MENU_TOGGLE_ON_TOP: &str = "toggle_on_top";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum OverlayAppearance {
    Glass,
    Solid,
}

impl Default for OverlayAppearance {
    fn default() -> Self {
        Self::Glass
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
struct OverlayStatePayload {
    visible: bool,
    locked: bool,
    always_on_top: bool,
    auto_center: bool,
    opacity: f64,
    appearance: OverlayAppearance,
    color: String,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}

impl Default for OverlayStatePayload {
    fn default() -> Self {
        Self {
            visible: true,
            locked: false,
            always_on_top: true,
            auto_center: false,
            opacity: 0.82,
            appearance: OverlayAppearance::Glass,
            color: "#0d1117".into(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            x: None,
            y: None,
        }
    }
}

#[derive(Debug)]
struct SharedState {
    overlay: OverlayStatePayload,
}

impl SharedState {
    fn new(overlay: OverlayStatePayload) -> Self {
        Self { overlay }
    }
}

struct AppState(Mutex<SharedState>);

fn sanitize_state(mut state: OverlayStatePayload) -> OverlayStatePayload {
    state.opacity = state.opacity.clamp(0.2, 1.0);
    state.width = state.width.max(MIN_WIDTH);
    state.height = state.height.max(MIN_HEIGHT);
    if !is_valid_hex_color(&state.color) {
        state.color = OverlayStatePayload::default().color;
    }
    state
}

fn is_valid_hex_color(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() == 7
        && chars[0] == '#'
        && chars[1..].iter().all(|c| c.is_ascii_hexdigit())
}

fn is_opacity_selected(current: f64, candidate: f64) -> bool {
    (current - candidate).abs() < 0.01
}

fn is_color_selected(current: &str, candidate: &str) -> bool {
    current.eq_ignore_ascii_case(candidate)
}

fn load_saved_state(app: &AppHandle) -> OverlayStatePayload {
    let store = match app.store(STORE_PATH) {
        Ok(store) => store,
        Err(_) => return OverlayStatePayload::default(),
    };

    let state = store
        .get(STORE_KEY)
        .and_then(|value| serde_json::from_value::<OverlayStatePayload>(value).ok())
        .unwrap_or_default();

    store.close_resource();
    sanitize_state(state)
}

fn save_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|err| err.to_string())?;
    store.set(STORE_KEY, json!(overlay));
    store.save().map_err(|err| err.to_string())?;
    store.close_resource();
    Ok(())
}

fn current_overlay_state(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let state = app.state::<AppState>();
    let shared = state.0.lock().map_err(|err| err.to_string())?;
    Ok(shared.overlay.clone())
}

fn refresh_tray_menu(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let menu = build_overlay_menu(app, overlay).map_err(|err| err.to_string())?;
    tray.set_menu(Some(menu)).map_err(|err| err.to_string())
}

fn emit_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    app.emit(EVENT_STATE_CHANGED, overlay.clone())
        .map_err(|err| err.to_string())
}

fn centered_horizontal_position(
    window: &tauri::WebviewWindow,
    overlay: &OverlayStatePayload,
) -> Result<Option<LogicalPosition<f64>>, String> {
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .or(window.primary_monitor().map_err(|err| err.to_string())?);

    let Some(monitor) = monitor else {
        return Ok(None);
    };

    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let work_x = f64::from(work_area.position.x) / scale;
    let work_y = f64::from(work_area.position.y) / scale;
    let work_width = f64::from(work_area.size.width) / scale;
    let work_height = f64::from(work_area.size.height) / scale;
    let centered_x = work_x + ((work_width - overlay.width) / 2.0);
    let y = overlay
        .y
        .unwrap_or_else(|| work_y + ((work_height - overlay.height) / 2.0));

    Ok(Some(LogicalPosition::new(centered_x, y)))
}

fn apply_window_state(app: &AppHandle, overlay: &OverlayStatePayload) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;

    window
        .set_size(LogicalSize::new(overlay.width, overlay.height))
        .map_err(|err| err.to_string())?;

    if overlay.auto_center {
        if let Some(position) = centered_horizontal_position(&window, overlay)? {
            window
                .set_position(position)
                .map_err(|err| err.to_string())?;
        } else {
            window.center().map_err(|err| err.to_string())?;
        }
    } else if let (Some(x), Some(y)) = (overlay.x, overlay.y) {
        window
            .set_position(LogicalPosition::new(x, y))
            .map_err(|err| err.to_string())?;
    } else {
        window.center().map_err(|err| err.to_string())?;
    }

    window
        .set_always_on_top(overlay.always_on_top)
        .map_err(|err| err.to_string())?;
    window
        .set_ignore_cursor_events(overlay.locked)
        .map_err(|err| err.to_string())?;
    window.set_effects(None::<tauri::utils::config::WindowEffectsConfig>)
        .map_err(|err| err.to_string())?;

    if overlay.visible {
        window.show().map_err(|err| err.to_string())?;
    } else {
        window.hide().map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn update_overlay_state<F>(
    app: &AppHandle,
    persist: bool,
    focus_window: bool,
    updater: F,
) -> Result<OverlayStatePayload, String>
where
    F: FnOnce(&mut SharedState),
{
    let overlay = {
        let state = app.state::<AppState>();
        let mut shared = state.0.lock().map_err(|err| err.to_string())?;
        updater(&mut shared);
        shared.overlay = sanitize_state(shared.overlay.clone());
        shared.overlay.clone()
    };

    apply_window_state(app, &overlay)?;

    if persist {
        save_state(app, &overlay)?;
    }

    refresh_tray_menu(app, &overlay)?;
    emit_state(app, &overlay)?;

    if focus_window && overlay.visible && !overlay.locked {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }

    Ok(overlay)
}

fn toggle_visibility(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.visible = !shared.overlay.visible;
    })
}

fn toggle_lock(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.locked = !shared.overlay.locked;
    })
}

fn enter_edit_mode_inner(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, true, |shared| {
        shared.overlay.visible = true;
        shared.overlay.locked = false;
    })
}

fn reset_overlay_position_inner(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, true, |shared| {
        shared.overlay.visible = true;
        shared.overlay.locked = false;
        shared.overlay.x = None;
        shared.overlay.y = None;
    })
}

fn toggle_on_top(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.always_on_top = !shared.overlay.always_on_top;
    })
}

fn toggle_auto_center(app: &AppHandle) -> Result<OverlayStatePayload, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let scale_factor = window.scale_factor().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;

    update_overlay_state(app, true, false, move |shared| {
        let next_auto_center = !shared.overlay.auto_center;
        shared.overlay.auto_center = next_auto_center;

        if next_auto_center {
            shared.overlay.x = None;
        } else {
            shared.overlay.x = Some(f64::from(position.x) / scale_factor);
        }

        shared.overlay.y = Some(f64::from(position.y) / scale_factor);
    })
}

fn set_opacity(app: &AppHandle, opacity: f64) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.opacity = opacity;
    })
}

fn set_appearance(app: &AppHandle, appearance: OverlayAppearance) -> Result<OverlayStatePayload, String> {
    update_overlay_state(app, true, false, |shared| {
        shared.overlay.appearance = appearance;
    })
}

fn set_color(app: &AppHandle, color: &str) -> Result<OverlayStatePayload, String> {
    let color = color.to_string();
    update_overlay_state(app, true, false, move |shared| {
        shared.overlay.color = color;
    })
}

fn build_overlay_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let visibility = MenuItem::with_id(
        app,
        MENU_TOGGLE_VISIBLE,
        if overlay.visible {
            "隐藏遮挡条"
        } else {
            "显示遮挡条"
        },
        true,
        Some("Ctrl+Alt+H"),
    )?;
    let lock = MenuItem::with_id(
        app,
        MENU_TOGGLE_LOCK,
        if overlay.locked {
            "解锁遮挡条"
        } else {
            "锁定遮挡条"
        },
        true,
        Some("Ctrl+Alt+Space"),
    )?;
    let edit_mode =
        MenuItem::with_id(app, MENU_EDIT_MODE, "进入编辑模式", true, None::<&str>)?;
    let auto_center = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_AUTO_CENTER,
        "自动水平居中",
        true,
        overlay.auto_center,
        None::<&str>,
    )?;
    let reset_position =
        MenuItem::with_id(app, MENU_RESET_POSITION, "重置位置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let always_on_top = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_ON_TOP,
        "始终置顶",
        true,
        overlay.always_on_top,
        None::<&str>,
    )?;

    let opacity_20 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_20,
        "20%",
        true,
        is_opacity_selected(overlay.opacity, 0.2),
        None::<&str>,
    )?;
    let opacity_40 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_40,
        "40%",
        true,
        is_opacity_selected(overlay.opacity, 0.4),
        None::<&str>,
    )?;
    let opacity_60 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_60,
        "60%",
        true,
        is_opacity_selected(overlay.opacity, 0.6),
        None::<&str>,
    )?;
    let opacity_80 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_80,
        "80%",
        true,
        is_opacity_selected(overlay.opacity, 0.8),
        None::<&str>,
    )?;
    let opacity_100 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_100,
        "100%",
        true,
        is_opacity_selected(overlay.opacity, 1.0),
        None::<&str>,
    )?;
    let opacity_items: [&dyn IsMenuItem<tauri::Wry>; 5] = [
        &opacity_20,
        &opacity_40,
        &opacity_60,
        &opacity_80,
        &opacity_100,
    ];
    let opacity_menu = Submenu::with_items(app, "不透明度", true, &opacity_items)?;

    let appearance_glass = CheckMenuItem::with_id(
        app,
        MENU_APPEARANCE_GLASS,
        "毛玻璃",
        true,
        overlay.appearance == OverlayAppearance::Glass,
        None::<&str>,
    )?;
    let appearance_solid = CheckMenuItem::with_id(
        app,
        MENU_APPEARANCE_SOLID,
        "纯色",
        true,
        overlay.appearance == OverlayAppearance::Solid,
        None::<&str>,
    )?;
    let appearance_items: [&dyn IsMenuItem<tauri::Wry>; 2] =
        [&appearance_glass, &appearance_solid];
    let appearance_menu = Submenu::with_items(app, "遮挡样式", true, &appearance_items)?;

    let color_black = CheckMenuItem::with_id(
        app,
        MENU_COLOR_BLACK,
        "黑色",
        true,
        is_color_selected(&overlay.color, "#0b0d12"),
        None::<&str>,
    )?;
    let color_charcoal = CheckMenuItem::with_id(
        app,
        MENU_COLOR_CHARCOAL,
        "深灰",
        true,
        is_color_selected(&overlay.color, "#2b3038"),
        None::<&str>,
    )?;
    let color_silver = CheckMenuItem::with_id(
        app,
        MENU_COLOR_SILVER,
        "浅灰",
        true,
        is_color_selected(&overlay.color, "#d0d5dd"),
        None::<&str>,
    )?;
    let color_white = CheckMenuItem::with_id(
        app,
        MENU_COLOR_WHITE,
        "白色",
        true,
        is_color_selected(&overlay.color, "#f8fafc"),
        None::<&str>,
    )?;
    let color_items: [&dyn IsMenuItem<tauri::Wry>; 4] =
        [&color_black, &color_charcoal, &color_silver, &color_white];
    let color_menu = Submenu::with_items(app, "颜色", true, &color_items)?;

    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 11] = [
        &visibility,
        &lock,
        &edit_mode,
        &auto_center,
        &separator_a,
        &opacity_menu,
        &appearance_menu,
        &color_menu,
        &separator_b,
        &always_on_top,
        &reset_position,
    ];
    let menu = Menu::with_items(app, &items)?;
    menu.append(&separator_c)?;
    menu.append(&quit)?;
    Ok(menu)
}

fn handle_menu_action(app: &AppHandle, id: &str) {
    let result = match id {
        MENU_TOGGLE_VISIBLE => toggle_visibility(app),
        MENU_TOGGLE_LOCK => toggle_lock(app),
        MENU_EDIT_MODE => enter_edit_mode_inner(app),
        MENU_TOGGLE_AUTO_CENTER => toggle_auto_center(app),
        MENU_OPACITY_20 => set_opacity(app, 0.2),
        MENU_OPACITY_40 => set_opacity(app, 0.4),
        MENU_OPACITY_60 => set_opacity(app, 0.6),
        MENU_OPACITY_80 => set_opacity(app, 0.8),
        MENU_OPACITY_100 => set_opacity(app, 1.0),
        MENU_APPEARANCE_GLASS => set_appearance(app, OverlayAppearance::Glass),
        MENU_APPEARANCE_SOLID => set_appearance(app, OverlayAppearance::Solid),
        MENU_COLOR_BLACK => set_color(app, "#0b0d12"),
        MENU_COLOR_CHARCOAL => set_color(app, "#2b3038"),
        MENU_COLOR_SILVER => set_color(app, "#d0d5dd"),
        MENU_COLOR_WHITE => set_color(app, "#f8fafc"),
        MENU_RESET_POSITION => reset_overlay_position_inner(app),
        MENU_TOGGLE_ON_TOP => toggle_on_top(app),
        MENU_QUIT => {
            app.exit(0);
            return;
        }
        _ => Ok(current_overlay_state(app).unwrap_or_default()),
    };

    if let Err(err) = result {
        eprintln!("menu action failed: {err}");
    }
}

#[tauri::command]
fn show_overlay_menu(app: AppHandle, x: f64, y: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到遮挡窗口".to_string())?;
    let overlay = current_overlay_state(&app)?;
    let menu = build_overlay_menu(&app, &overlay).map_err(|err| err.to_string())?;

    window
        .popup_menu_at(&menu, LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn load_overlay_state(app: AppHandle) -> Result<OverlayStatePayload, String> {
    current_overlay_state(&app)
}

#[tauri::command]
fn apply_overlay_state(
    app: AppHandle,
    next_state: OverlayStatePayload,
) -> Result<OverlayStatePayload, String> {
    update_overlay_state(&app, true, false, move |shared| {
        shared.overlay = next_state;
    })
}

#[tauri::command]
fn toggle_overlay_visibility(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_visibility(&app)
}

#[tauri::command]
fn toggle_overlay_lock(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_lock(&app)
}

#[tauri::command]
fn enter_edit_mode(app: AppHandle) -> Result<OverlayStatePayload, String> {
    enter_edit_mode_inner(&app)
}

#[tauri::command]
fn toggle_overlay_on_top(app: AppHandle) -> Result<OverlayStatePayload, String> {
    toggle_on_top(&app)
}

#[tauri::command]
fn reset_overlay_position(app: AppHandle) -> Result<OverlayStatePayload, String> {
    reset_overlay_position_inner(&app)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay = current_overlay_state(app).map_err(|err| err.to_string())?;
    let menu = build_overlay_menu(app, &overlay)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .ok_or("missing default window icon")?
                .clone(),
        )
        .tooltip("字幕遮挡")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Err(err) = toggle_visibility(&app) {
                    eprintln!("tray left click failed: {err}");
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle_visibility_shortcut = Shortcut::try_from("ctrl+alt+h")?;
    let toggle_lock_shortcut = Shortcut::try_from("ctrl+alt+space")?;

    let toggle_visibility_for_handler = toggle_visibility_shortcut;
    let toggle_lock_for_handler = toggle_lock_shortcut;

    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_shortcuts([toggle_visibility_shortcut, toggle_lock_shortcut])?
            .with_handler(move |app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }

                let result = if shortcut == &toggle_visibility_for_handler {
                    toggle_visibility(app)
                } else if shortcut == &toggle_lock_for_handler {
                    toggle_lock(app)
                } else {
                    return;
                };

                if let Err(err) = result {
                    eprintln!("shortcut handler failed: {err}");
                }
            })
            .build(),
    )?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .on_menu_event(|app, event| {
            handle_menu_action(app, event.id.as_ref());
        })
        .setup(|app| {
            let overlay = load_saved_state(app.handle());
            app.manage(AppState(Mutex::new(SharedState::new(overlay.clone()))));

            apply_window_state(app.handle(), &overlay)?;
            emit_state(app.handle(), &overlay)?;
            setup_tray(app.handle())?;

            #[cfg(desktop)]
            setup_shortcuts(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_overlay_state,
            apply_overlay_state,
            toggle_overlay_visibility,
            toggle_overlay_lock,
            enter_edit_mode,
            toggle_overlay_on_top,
            reset_overlay_position,
            show_overlay_menu,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
