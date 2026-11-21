mod bridge;
mod commands;
mod desktop;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::create_room,
            commands::join_room,
            commands::get_snapshot,
            commands::disconnect,
            commands::send_message,
            commands::send_reaction,
            commands::rekey_now,
            commands::send_over,
        ])
        .setup(|app| {
            desktop::setup(app)?;
            Ok(())
        })
        .on_window_event(desktop::handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running SecureChat");
}
