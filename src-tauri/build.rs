fn main() {
    // tauri-build は Windows のアプリケーションマニフェスト（Common Controls v6 への依存）を
    // 実行ファイルにしか埋め込まない。テストの実行ファイルにも必要なので（ないと Tauri の
    // コードを含むテストが STATUS_ENTRYPOINT_NOT_FOUND で起動できない）、リンカーで全体に埋め込む。
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    embed_windows_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("tauri-build に失敗しました");
}

fn embed_windows_manifest() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // マニフェストの警告（重複など）を見逃さないようにする
    println!("cargo:rustc-link-arg=/WX");
}
