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
        assert!(parse("/conflict/1/other").is_none());
        assert!(parse("/unknown/1/1").is_none());
        assert!(parse("/preview/1").is_none());
    }

    use crate::decoder::tests::write_dummy;
    use crate::fileops::{self, Action, Session};
    use crate::state::SessionState;
    use std::sync::Arc;

    /// 2 枚の画像で仕分けを始めた状態（番号 1）を作る
    fn state_with_session(dir: &std::path::Path) -> AppState {
        let images: Vec<PathBuf> = ["a.png", "b.png"].iter().map(|n| dir.join(n)).collect();
        for p in &images {
            write_dummy(p, 300, 200);
        }
        let state = AppState::new(dir.join("config.toml"));
        let session = Session::new(Arc::new(fileops::RealFs), images, None);
        *state.session() =
            Some(SessionState { session, generation: 1, source: dir.to_path_buf(), unsupported: Vec::new() });
        state
    }

    fn size(enc: &Encoded) -> (u32, u32) {
        let img = image::load_from_memory(&enc.bytes).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn serves_preview_and_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_session(dir.path());
        let full = serve(&state, "/preview/1/0").unwrap();
        assert_eq!(full.mime, "image/jpeg");
        assert_eq!(size(&full), (300, 200));
        assert_eq!(size(&serve(&state, "/thumb/1/1").unwrap()), (240, 160));
    }

    #[test]
    fn rejects_stale_or_unknown_requests() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_session(dir.path());
        assert!(serve(&state, "/preview/2/0").unwrap_err().contains("古い画面"));
        assert!(serve(&state, "/preview/1/5").unwrap_err().contains("見つかりません"));
        assert!(serve(&state, "/nope").unwrap_err().contains("不正"));
        assert!(serve(&state, "/conflict/1/incoming").unwrap_err().contains("確認中ではありません"));
    }

    #[test]
    fn serves_moved_image_and_both_sides_of_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_session(dir.path());
        let target = dir.path().join("t");
        std::fs::create_dir(&target).unwrap();
        // 移動した後も、移動先から表示できる
        state
            .session()
            .as_mut()
            .unwrap()
            .session
            .apply(Action::MoveTo { dir: target.clone(), label: "t".into() })
            .unwrap();
        assert!(!dir.path().join("a.png").exists());
        assert_eq!(size(&serve(&state, "/preview/1/0").unwrap()), (300, 200));

        write_dummy(&target.join("b.png"), 50, 40);
        state.session().as_mut().unwrap().session.apply(Action::MoveTo { dir: target, label: "t".into() }).unwrap();
        assert_eq!(size(&serve(&state, "/conflict/1/incoming").unwrap()), (300, 200));
        assert_eq!(size(&serve(&state, "/conflict/1/existing").unwrap()), (50, 40));
    }
}
