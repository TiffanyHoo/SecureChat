use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, UserAttentionType, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;

const BASE_TITLE: &str = "SecureChat 安全通话";
static UNREAD: AtomicU32 = AtomicU32::new(0);
static TRAY_TIP_SHOWN: AtomicBool = AtomicBool::new(false);

pub fn setup(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "显示 SecureChat", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/app-icon-32.png"))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .icon_as_template(false)
        .tooltip(BASE_TITLE)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => crate::commands::shutdown(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = window.hide();
            if !TRAY_TIP_SHOWN.swap(true, Ordering::Relaxed) {
                let _ = window
                    .app_handle()
                    .notification()
                    .builder()
                    .title(BASE_TITLE)
                    .body("已最小化到托盘，后台仍会接收加密消息。")
                    .show();
            }
        }
        WindowEvent::Focused(true) => reset_notifications(window.app_handle()),
        _ => {}
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    reset_notifications(app);
}

pub fn notify_incoming(app: &AppHandle, sender: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_focused().unwrap_or(true) {
        reset_notifications(app);
        return;
    }

    let unread = UNREAD.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = window.set_title(&format!("{BASE_TITLE} - {unread} 条未读"));
    let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    let _ = app
        .notification()
        .builder()
        .title("SecureChat 新消息")
        .body(format!("{sender} 发来一条加密消息"))
        .show();
    play_message_sound();
}

fn reset_notifications(app: &AppHandle) {
    UNREAD.store(0, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(BASE_TITLE);
        let _ = window.request_user_attention(None);
    }
}

#[cfg(windows)]
fn play_message_sound() {
    unsafe {
        let _ = windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0);
    }
}

#[cfg(not(windows))]
fn play_message_sound() {}
