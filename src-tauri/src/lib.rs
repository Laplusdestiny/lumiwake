pub mod commands;
pub mod config;
pub mod decoder;
pub mod fileops;
pub mod format;
pub mod protocol;
pub mod scanner;
pub mod state;
pub mod suggest;

use state::AppState;
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager, WindowEvent};

/// 閉じるボタンなどでウィンドウを閉じようとしたときに画面へ送るイベント
pub const CLOSE_REQUESTED_EVENT: &str = "lumiwake://close-requested";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.toml");
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState::new(config_path, data_dir));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 削除予定の確認が済むまでは閉じず、画面に確認ダイアログを出させる
                let allowed = window.state::<AppState>().allow_exit.load(Ordering::SeqCst);
                if !allowed {
                    api.prevent_close();
                    let _ = window.emit(CLOSE_REQUESTED_EVENT, ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::validate_config,
            commands::save_config,
            commands::reload_config,
            commands::set_view_mode,
            commands::set_show_paths,
            commands::open_config_file,
            commands::open_location,
            commands::list_subfolders,
            commands::start_session,
            commands::get_session,
            commands::perform,
            commands::resolve_conflict,
            commands::cancel_conflict,
            commands::undo,
            commands::navigate,
            commands::jump_to,
            commands::image_info,
            commands::get_suggestions,
            commands::ai_key_status,
            commands::test_systemone,
            commands::set_external_consent,
            commands::pending_deletions,
            commands::finalize_and_exit,
            commands::exit_app,
        ])
        .run(tauri::generate_context!())
        .expect("Lumiwake の起動に失敗しました");
}
