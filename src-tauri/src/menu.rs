use super::*;

pub(super) fn build_overlay_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    settings: &SubtitleExplainSettingsPayload,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let visibility = MenuItem::with_id(
        app,
        MENU_TOGGLE_VISIBLE,
        if overlay.visible { "隐藏" } else { "显示" },
        true,
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_visibility),
    )?;
    let _lock_legacy = MenuItem::with_id(
        app,
        "toggle_lock_legacy_unused",
        if overlay.locked { "解锁" } else { "锁定" },
        true,
        None::<&str>,
    )?;
    let lock = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_LOCK,
        "锁定",
        true,
        overlay.locked,
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_lock),
    )?;
    let click_through = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_CLICK_THROUGH,
        "锁定时点击穿透",
        true,
        overlay.click_through_when_locked,
        None::<&str>,
    )?;
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
    let always_on_top = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_ON_TOP,
        "始终置顶",
        true,
        overlay.always_on_top,
        None::<&str>,
    )?;

    let opacity_50 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_50,
        "50%",
        true,
        is_opacity_selected(overlay.opacity, 0.5),
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
    let opacity_70 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_70,
        "70%",
        true,
        is_opacity_selected(overlay.opacity, 0.7),
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
    let opacity_90 = CheckMenuItem::with_id(
        app,
        MENU_OPACITY_90,
        "90%",
        true,
        is_opacity_selected(overlay.opacity, 0.9),
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
    let opacity_items: [&dyn IsMenuItem<tauri::Wry>; 6] = [
        &opacity_50,
        &opacity_60,
        &opacity_70,
        &opacity_80,
        &opacity_90,
        &opacity_100,
    ];
    let opacity_menu = Submenu::with_items(app, "不透明度", true, &opacity_items)?;

    let refresh_rate_5 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_5,
        "5 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 5),
        None::<&str>,
    )?;
    let refresh_rate_10 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_10,
        "10 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 10),
        None::<&str>,
    )?;
    let refresh_rate_15 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_15,
        "15 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 15),
        None::<&str>,
    )?;
    let refresh_rate_20 = CheckMenuItem::with_id(
        app,
        MENU_REFRESH_RATE_20,
        "20 FPS",
        true,
        is_refresh_rate_selected(overlay.refresh_rate, 20),
        None::<&str>,
    )?;
    let refresh_rate_items: [&dyn IsMenuItem<tauri::Wry>; 4] = [
        &refresh_rate_5,
        &refresh_rate_10,
        &refresh_rate_15,
        &refresh_rate_20,
    ];
    let refresh_rate_menu = Submenu::with_items(
        app,
        "刷新率",
        overlay.appearance == OverlayAppearance::Pixelate,
        &refresh_rate_items,
    )?;

    let pixel_detail_coarse = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_COARSE,
        "粗",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Coarse),
        None::<&str>,
    )?;
    let pixel_detail_medium = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_MEDIUM,
        "中",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Medium),
        None::<&str>,
    )?;
    let pixel_detail_fine = CheckMenuItem::with_id(
        app,
        MENU_PIXEL_DETAIL_FINE,
        "细",
        true,
        is_pixel_detail_selected(&overlay.pixel_detail, &PixelDetail::Fine),
        None::<&str>,
    )?;
    let pixel_detail_items: [&dyn IsMenuItem<tauri::Wry>; 3] = [
        &pixel_detail_coarse,
        &pixel_detail_medium,
        &pixel_detail_fine,
    ];
    let pixel_detail_menu = Submenu::with_items(
        app,
        "像素块",
        overlay.appearance == OverlayAppearance::Pixelate,
        &pixel_detail_items,
    )?;

    let appearance_pixelate = CheckMenuItem::with_id(
        app,
        MENU_APPEARANCE_PIXELATE,
        "像素化",
        true,
        overlay.appearance == OverlayAppearance::Pixelate,
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
        [&appearance_pixelate, &appearance_solid];
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
    let color_menu = Submenu::with_items(
        app,
        "颜色",
        overlay.appearance == OverlayAppearance::Solid,
        &color_items,
    )?;

    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let _separator_d = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 13] = [
        &visibility,
        &lock,
        &click_through,
        &auto_center,
        &separator_a,
        &appearance_menu,
        &refresh_rate_menu,
        &pixel_detail_menu,
        &opacity_menu,
        &color_menu,
        &separator_b,
        &always_on_top,
        &reset_position,
    ];
    let menu = Menu::with_items(app, &items)?;
    Ok(menu)
}

pub(super) fn build_tray_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
    settings: &SubtitleExplainSettingsPayload,
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
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_visibility),
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
        shortcut_accelerator(&settings.shortcut_settings.toggle_overlay_lock),
    )?;
    let reset_position = MenuItem::with_id(
        app,
        MENU_RESET_POSITION,
        "重置遮挡条位置",
        true,
        None::<&str>,
    )?;
    let show_subtitle_explain = MenuItem::with_id(
        app,
        MENU_SHOW_SUBTITLE_EXPLAIN,
        "打开字幕助手",
        true,
        None::<&str>,
    )?;
    let more_settings =
        MenuItem::with_id(app, MENU_OPEN_MORE_SETTINGS, "更多设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;

    let items: [&dyn IsMenuItem<tauri::Wry>; 8] = [
        &visibility,
        &lock,
        &reset_position,
        &separator_a,
        &show_subtitle_explain,
        &separator_b,
        &more_settings,
        &separator_c,
    ];
    let menu = Menu::with_items(app, &items)?;
    menu.append(&quit)?;
    Ok(menu)
}

pub(super) fn refresh_tray_menu(
    app: &AppHandle,
    overlay: &OverlayStatePayload,
) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let settings = current_subtitle_settings(app)?;
    let menu = build_tray_menu(app, overlay, &settings).map_err(|err| err.to_string())?;
    tray.set_menu(Some(menu)).map_err(|err| err.to_string())
}

pub(super) fn handle_menu_action(app: &AppHandle, id: &str) {
    let result = match id {
        MENU_TOGGLE_VISIBLE => super::overlay::toggle_visibility(app),
        MENU_ANALYZE_CURRENT_SUBTITLE => {
            super::subtitle::start_manual_subtitle_analysis(app.clone());
            Ok(current_overlay_state(app).unwrap_or_default())
        }
        MENU_EDIT_OCR_REGION => {
            let _ = super::app_window::toggle_ocr_region_editor(app);
            Ok(current_overlay_state(app).unwrap_or_default())
        }
        MENU_SHOW_SUBTITLE_EXPLAIN => {
            let _ = super::app_window::open_subtitle_explain_window(app);
            Ok(current_overlay_state(app).unwrap_or_default())
        }
        MENU_TOGGLE_LOCK => super::overlay::toggle_lock(app),
        MENU_TOGGLE_CLICK_THROUGH => super::overlay::toggle_click_through(app),
        MENU_TOGGLE_AUTO_CENTER => super::overlay::toggle_auto_center(app),
        MENU_REFRESH_RATE_5 => super::overlay::set_refresh_rate(app, 5),
        MENU_REFRESH_RATE_10 => super::overlay::set_refresh_rate(app, 10),
        MENU_REFRESH_RATE_15 => super::overlay::set_refresh_rate(app, 15),
        MENU_REFRESH_RATE_20 => super::overlay::set_refresh_rate(app, 20),
        MENU_PIXEL_DETAIL_COARSE => super::overlay::set_pixel_detail(app, PixelDetail::Coarse),
        MENU_PIXEL_DETAIL_MEDIUM => super::overlay::set_pixel_detail(app, PixelDetail::Medium),
        MENU_PIXEL_DETAIL_FINE => super::overlay::set_pixel_detail(app, PixelDetail::Fine),
        MENU_OPACITY_50 => super::overlay::set_opacity(app, 0.5),
        MENU_OPACITY_60 => super::overlay::set_opacity(app, 0.6),
        MENU_OPACITY_70 => super::overlay::set_opacity(app, 0.7),
        MENU_OPACITY_80 => super::overlay::set_opacity(app, 0.8),
        MENU_OPACITY_90 => super::overlay::set_opacity(app, 0.9),
        MENU_OPACITY_100 => super::overlay::set_opacity(app, 1.0),
        MENU_APPEARANCE_PIXELATE => {
            super::overlay::set_appearance(app, OverlayAppearance::Pixelate)
        }
        MENU_APPEARANCE_SOLID => super::overlay::set_appearance(app, OverlayAppearance::Solid),
        MENU_COLOR_BLACK => super::overlay::set_color(app, "#0b0d12"),
        MENU_COLOR_CHARCOAL => super::overlay::set_color(app, "#2b3038"),
        MENU_COLOR_SILVER => super::overlay::set_color(app, "#d0d5dd"),
        MENU_COLOR_WHITE => super::overlay::set_color(app, "#f8fafc"),
        MENU_RESET_POSITION => super::overlay::reset_overlay_position_inner(app),
        MENU_TOGGLE_ON_TOP => super::overlay::toggle_on_top(app),
        MENU_OPEN_MORE_SETTINGS => super::app_window::open_more_settings_menu(app),
        MENU_QUIT => {
            super::app_window::sync_persisted_window_states(app);
            app.exit(0);
            return;
        }
        _ => Ok(current_overlay_state(app).unwrap_or_default()),
    };

    if let Err(err) = result {
        eprintln!("menu action failed: {err}");
    }
}

pub(super) fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay = current_overlay_state(app).map_err(|err| err.to_string())?;
    let settings = current_subtitle_settings(app).map_err(|err| err.to_string())?;
    let menu = build_tray_menu(app, &overlay, &settings)?;

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
                let result = match current_subtitle_settings(&app)
                    .map(|settings| settings.tray_click_action)
                    .unwrap_or_default()
                {
                    TrayClickAction::Overlay => super::overlay::toggle_visibility(&app).map(|_| ()),
                    TrayClickAction::SubtitleExplain => {
                        super::app_window::toggle_subtitle_explain_window(&app)
                    }
                };

                if let Err(err) = result {
                    eprintln!("tray left click failed: {err}");
                }
            }
        })
        .build(app)?;

    Ok(())
}
