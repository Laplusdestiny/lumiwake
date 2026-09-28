//! 画像を WebView に渡すカスタムプロトコル `lumi://`。
//!
//! WebView には任意のファイルパスではなく「セッション番号＋画像番号」だけを渡し、
//! Rust 側でデコード・縮小した画像を返す。
//!
//! - `/preview/{generation}/{index}` 表示用（最大 2560px）
//! - `/thumb/{generation}/{index}` フィルムストリップ用
//! - `/conflict/{generation}/{incoming|existing}` 同名ファイルの比較用

use crate::decoder::{self, Encoded};
use crate::state::{location, AppState};
use std::path::PathBuf;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

pub const SCHEME: &str = "lumi";

pub fn handle<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app = ctx.app_handle().clone();
    // デコードは重いので別スレッドで行い、WebView のスレッドを止めない
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let response = match serve(&state, request.uri().path()) {
            Ok(enc) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, enc.mime)
                .header(header::CACHE_CONTROL, "no-store")
                .body(enc.bytes),
            Err(message) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(message.into_bytes()),
        };
        responder.respond(response.expect("レスポンスを組み立てられません"));
    });
}

enum Route {
    Preview(usize),
    Thumb(usize),
    Conflict { incoming: bool },
}

fn parse(path: &str) -> Option<(u64, Route)> {
    let mut parts = path.trim_matches('/').split('/');
    let kind = parts.next()?;
    let generation = parts.next()?.parse().ok()?;
    let arg = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let req = match kind {
        "preview" => Route::Preview(arg.parse().ok()?),
        "thumb" => Route::Thumb(arg.parse().ok()?),
        "conflict" => Route::Conflict {
            incoming: match arg {
                "incoming" => true,
                "existing" => false,
                _ => return None,
            },
        },
        _ => return None,
    };
    Some((generation, req))
}

fn serve(state: &AppState, path: &str) -> Result<Encoded, String> {
    let (generation, req) = parse(path).ok_or("不正な URL です")?;
    let (key, file): (Option<PathBuf>, PathBuf) = {
        let guard = state.session();
        let s = guard.as_ref().filter(|s| s.generation == generation).ok_or("古い画面からの要求です")?;
        match req {
            Route::Preview(i) | Route::Thumb(i) => {
                let item = s.session.items().get(i).ok_or("画像が見つかりません")?;
                (Some(item.path.clone()), location(&item.path, &item.status).to_path_buf())
            }
            Route::Conflict { incoming } => {
                let c = s.session.pending_conflict().ok_or("同名ファイルの確認中ではありません")?;
                (None, if incoming { c.incoming.clone() } else { c.existing.clone() })
            }
        }
    };
    match key {
        // 仕分け対象の画像はキャッシュ（先読み）を使う
        Some(key) => {
            let preview = state.cache.get_or_load(&key, || decoder::make_preview(&state.registry, &file))?;
            Ok(if matches!(req, Route::Thumb(_)) { preview.thumb.clone() } else { preview.full.clone() })
        }
        // 比較用の画像は内容が入れ替わりうるのでキャッシュしない
        None => Ok(decoder::make_preview(&state.registry, &file)?.full),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        assert!(matches!(parse("/preview/3/12"), Some((3, Route::Preview(12)))));
        assert!(matches!(parse("/thumb/1/0"), Some((1, Route::Thumb(0)))));
        assert!(matches!(parse("/conflict/2/existing"), Some((2, Route::Conflict { incoming: false }))));
        assert!(parse("/preview/x/1").is_none());
        assert!(parse("/preview/1/1/extra").is_none());
        assert!(parse("/../../etc/passwd").is_none());
    }
}
