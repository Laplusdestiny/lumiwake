// リリースビルドの Windows で余計なコンソールウィンドウを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lumiwake_lib::run()
}
