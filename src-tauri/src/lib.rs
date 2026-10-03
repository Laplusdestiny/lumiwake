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

/// WebView 由来のショートカットのうち、無効にするもの。
/// 再読み込み・検索・印刷・ソース表示などが、振り分けキーや Lumiwake の操作と衝突しないようにする。
/// 設定画面を Tab / Shift+Tab で操作できるよう、フォーカス移動だけは残す。
/// デバッグビルドでは開発者ツール・再読み込み・右クリックメニューも残す。
fn prevent_default_flags(debug: bool) -> tauri_plugin_prevent_default::Flags {
    use tauri_plugin_prevent_default::Flags;
    let mut flags = Flags::all().difference(Flags::FOCUS_MOVE);
    if debug {
        flags = flags.difference(Flags::CONTEXT_MENU | Flags::DEV_TOOLS | Flags::RELOAD);
    }
    flags
}

fn prevent_default_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder =
        tauri_plugin_prevent_default::Builder::new().with_flags(prevent_default_flags(cfg!(debug_assertions)));
    // Windows（WebView2）では、ブラウザのアクセラレータキーも無効にする（デバッグビルドでは開発者ツールのため残す）
    #[cfg(windows)]
    let builder = builder.platform(
        tauri_plugin_prevent_default::PlatformOptions::new()
            .browser_accelerator_keys(cfg!(debug_assertions))
            .default_context_menus(cfg!(debug_assertions)),
    );
    builder.build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(prevent_default_plugin())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_prevent_default::Flags;

    #[test]
    fn release_disables_webview_shortcuts_but_keeps_tab_navigation() {
        let f = prevent_default_flags(false);
        for must in [Flags::RELOAD, Flags::FIND, Flags::PRINT, Flags::SOURCE, Flags::DEV_TOOLS, Flags::CONTEXT_MENU] {
            assert!(f.contains(must), "{must:?}");
        }
        assert!(!f.contains(Flags::FOCUS_MOVE), "設定画面を Shift+Tab で操作できるように残す");
    }

    #[test]
    fn debug_keeps_devtools_reload_and_context_menu() {
        let f = prevent_default_flags(true);
        for kept in [Flags::DEV_TOOLS, Flags::RELOAD, Flags::CONTEXT_MENU] {
            assert!(!f.contains(kept), "{kept:?}");
        }
        assert!(f.contains(Flags::PRINT) && f.contains(Flags::FIND), "それ以外は無効のまま");
    }
}
