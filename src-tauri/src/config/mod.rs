//! 設定管理: キー割り当てと各種設定を TOML で読み書きする。
//!
//! GUI の設定画面と TOML ファイルの両方から編集でき、GUI での変更は TOML に書き戻す。

pub mod keys;

use keys::{reserved_reason, KeyCombo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// 終了時に削除予定のファイルをどう扱うか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnExit {
    /// 完全削除するか確認する（既定）
    #[default]
    Confirm,
    /// 確認せずにそのまま完全削除する
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    /// 中央プレビュー＋右に振り分け先リスト（既定）
    #[default]
    Sidebar,
    /// 全画面集中型
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Amber,
    Blue,
    Green,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// 前回使った仕分け元フォルダ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_dir: Option<PathBuf>,
    /// サブフォルダも読み込むか（読み込み時に毎回変更できる）
    pub include_subdirs: bool,
    /// 削除フォルダ。未指定なら削除キーでファイルを動かさず、削除予定として記録する
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_folder: Option<PathBuf>,
    pub on_exit: OnExit,
    pub view_mode: ViewMode,
    pub accent: Accent,
    /// 先読みする枚数
    pub prefetch: usize,
    /// 起動時に更新を確認する
    pub check_updates: bool,
    /// 振り分け先リストにフォルダのパスを表示する（同名フォルダは非表示でもパスを出す）
    pub show_paths: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            source_dir: None,
            include_subdirs: false,
            delete_folder: None,
            on_exit: OnExit::Confirm,
            view_mode: ViewMode::Sidebar,
            accent: Accent::Amber,
            prefetch: 4,
            check_updates: true,
            show_paths: true,
        }
    }
}

/// 振り分け以外の操作キー
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionKeys {
    pub skip: String,
    pub delete: String,
    pub undo: String,
    pub prev: String,
    pub next: String,
    /// 表示モード（サイドバー／全画面）の切り替え
    pub toggle_view: String,
    /// 振り分け先リストのパス表示の切り替え
    pub toggle_paths: String,
}

impl Default for ActionKeys {
    fn default() -> Self {
        ActionKeys {
            skip: "Space".into(),
            delete: "Delete".into(),
            undo: "Ctrl+Z".into(),
            prev: "Left".into(),
            next: "Right".into(),
            // 単独キーは振り分けに使えるよう空けておき、表示の切り替えは組み合わせキーにする
            toggle_view: "Ctrl+Shift+F".into(),
            toggle_paths: "Ctrl+Shift+P".into(),
        }
    }
}

impl ActionKeys {
    fn entries(&self) -> [(&'static str, &str); 7] {
        [
            ("スキップ", &self.skip),
            ("削除", &self.delete),
            ("取り消し", &self.undo),
            ("前の画像", &self.prev),
            ("次の画像", &self.next),
            ("表示モード切り替え", &self.toggle_view),
            ("パス表示の切り替え", &self.toggle_paths),
        ]
    }

    fn entries_mut(&mut self) -> [&mut String; 7] {
        [
            &mut self.skip,
            &mut self.delete,
            &mut self.undo,
            &mut self.prev,
            &mut self.next,
            &mut self.toggle_view,
            &mut self.toggle_paths,
        ]
    }
}

/// 振り分け先フォルダ
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub key: String,
    /// 表示名。省略時はフォルダ名
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub path: PathBuf,
}

impl Target {
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}

/// 設定ファイルの形式のバージョン。既定値を変えたときの移行に使う
pub const CONFIG_VERSION: u32 = 1;

fn legacy_version() -> u32 {
    0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 書いていない古いファイルは 0 として読み、[`Config::migrate`] で最新にそろえる
    #[serde(default = "legacy_version")]
    pub version: u32,
    pub general: General,
    pub keys: ActionKeys,
    pub targets: Vec<Target>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            general: General::default(),
            keys: ActionKeys::default(),
            targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 保存できない
    Error,
    /// 保存はできるが注意が必要
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
    /// 問題のあるキー（正規形）。画面で該当行を強調するのに使う
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl Issue {
    fn error(message: String, key: Option<String>) -> Self {
        Issue { severity: Severity::Error, message, key }
    }
    fn warning(message: String, key: Option<String>) -> Self {
        Issue { severity: Severity::Warning, message, key }
    }
}

impl Config {
    /// キー表記を正規形にそろえ、空のパスを「未指定」に直す
    pub fn normalize(&mut self) {
        for k in self.keys.entries_mut() {
            if let Some(c) = KeyCombo::parse(k) {
                *k = c.to_string();
            }
        }
        for t in &mut self.targets {
            if let Some(c) = KeyCombo::parse(&t.key) {
                t.key = c.to_string();
            }
            t.name = t.name.trim().to_string();
        }
        let empty = |p: &Option<PathBuf>| p.as_ref().is_some_and(|p| p.as_os_str().is_empty());
        if empty(&self.general.delete_folder) {
            self.general.delete_folder = None;
        }
        if empty(&self.general.source_dir) {
            self.general.source_dir = None;
        }
        self.general.prefetch = self.general.prefetch.clamp(0, 16);
    }

    /// キーの重複・予約キーとの衝突・パスの問題を調べる
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        // 正規形のキー → 使っている場所
        let mut used: HashMap<String, Vec<String>> = HashMap::new();
        let mut check_key = |raw: &str, owner: String, issues: &mut Vec<Issue>| match KeyCombo::parse(raw) {
            None => issues.push(Issue::error(format!("{owner}のキー「{raw}」を解釈できません"), None)),
            Some(c) => {
                let s = c.to_string();
                if let Some(reason) = reserved_reason(&c) {
                    issues.push(Issue::warning(
                        format!("{owner}のキー {s} は予約されています（{reason}）。効かない場合があります"),
                        Some(s.clone()),
                    ));
                }
                used.entry(s).or_default().push(owner);
            }
        };
        for (label, key) in self.keys.entries() {
            check_key(key, format!("「{label}」"), &mut issues);
        }
        for t in &self.targets {
            check_key(&t.key, format!("振り分け先「{}」", t.display_name()), &mut issues);
        }
        let mut dups: Vec<_> = used.into_iter().filter(|(_, owners)| owners.len() > 1).collect();
        dups.sort();
        for (key, owners) in dups {
            issues.push(Issue::error(format!("キー {key} が重複しています: {}", owners.join("、")), Some(key)));
        }

        let delete_folder = self.general.delete_folder.as_deref();
        for t in &self.targets {
            let name = t.display_name();
            if t.path.as_os_str().is_empty() {
                issues.push(Issue::error(format!("振り分け先「{name}」のフォルダが指定されていません"), None));
                continue;
            }
            if !t.path.is_dir() {
                issues.push(Issue::warning(
                    format!("振り分け先「{name}」のフォルダが見つかりません: {}", t.path.display()),
                    None,
                ));
            }
            if delete_folder.is_some_and(|d| same_path(d, &t.path)) {
                issues.push(Issue::error(format!("振り分け先「{name}」が削除フォルダと同じです"), None));
            }
        }
        if let Some(d) = delete_folder {
            if !d.is_dir() {
                issues.push(Issue::warning(format!("削除フォルダが見つかりません: {}", d.display()), None));
            }
        }
        issues
    }

    pub fn has_errors(issues: &[Issue]) -> bool {
        issues.iter().any(|i| i.severity == Severity::Error)
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

const HEADER: &str = "\
# Lumiwake の設定ファイル
# アプリの設定画面から変更すると、このファイルは上書きされます（コメントは保持されません）。
# 手で編集した場合は、設定画面の「ファイルから再読み込み」かアプリの再起動で反映されます。
#
# キーの書き方: \"1\"、\"A\"、\"Ctrl+1\"、\"Shift+Alt+F2\"、\"Space\"、\"Delete\"、\"Left\"、\"Num1\"（テンキー）など
# on_exit: \"confirm\"（終了時に完全削除するか確認）/ \"delete\"（確認せずに完全削除）
# delete_folder を書かない場合、削除キーではファイルを動かさず「削除予定」として記録します。
#
# 振り分け先の例:
# [[targets]]
# key = \"1\"
# name = \"風景\"
# path = \"/home/me/Pictures/風景\"

";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("設定ファイルを読み込めません（{path}）: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("設定ファイルの書式に誤りがあります（{path}）: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("設定ファイルに書き込めません（{path}）: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("設定に誤りがあるため保存できません: {0}")]
    Invalid(String),
}

pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
    let mut config: Config =
        toml::from_str(text).map_err(|e| ConfigError::Parse { path: path.to_path_buf(), message: e.to_string() })?;
    config.normalize();
    config.migrate();
    Ok(config)
}

impl Config {
    /// 古い形式の設定を最新にそろえる（次に保存したときにファイルへ反映される）
    pub fn migrate(&mut self) {
        if self.version < 1 {
            // v0 の既定だった単独キー F は振り分けに使いたいので、組み合わせキーへ移す。
            // 自分で別のキーに変えていた場合はそのまま
            if self.keys.toggle_view == "F" {
                self.keys.toggle_view = ActionKeys::default().toggle_view;
            }
        }
        self.version = CONFIG_VERSION;
    }
}

pub fn to_toml(config: &Config) -> String {
    let body = toml::to_string_pretty(config).expect("設定は常に TOML に変換できる");
    format!("{HEADER}{body}")
}

/// 設定ファイルを読み込む。ファイルがなければ既定値で作成する。
pub fn load_or_create(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let config = Config::default();
            write_file(path, &config)?;
            Ok(config)
        }
        Err(source) => Err(ConfigError::Read { path: path.to_path_buf(), source }),
    }
}

/// 正規化・検証してから書き込む。エラーがあれば書き込まない。
pub fn save(path: &Path, config: &Config) -> Result<(Config, Vec<Issue>), ConfigError> {
    let mut config = config.clone();
    config.normalize();
    let issues = config.validate();
    if Config::has_errors(&issues) {
        let messages: Vec<_> =
            issues.iter().filter(|i| i.severity == Severity::Error).map(|i| i.message.clone()).collect();
        return Err(ConfigError::Invalid(messages.join(" / ")));
    }
    write_file(path, &config)?;
    Ok((config, issues))
}

/// ファイルを読み直して一部の項目だけを書き換える（前回の仕分け元・表示モードなど）。
/// 手で編集した内容をメモリ上の古い設定で上書きしないよう、必ずファイルの内容を土台にする。
/// ファイルに誤りがある場合は何も書き込まない。
pub fn update_file(path: &Path, patch: impl FnOnce(&mut Config)) -> Result<Config, ConfigError> {
    let mut config = load_or_create(path)?;
    patch(&mut config);
    save(path, &config).map(|(c, _)| c)
}

/// 一時ファイルに書いてから置き換え、書き込み途中で壊れないようにする
fn write_file(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let err = |source| ConfigError::Write { path: path.to_path_buf(), source };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, to_toml(config)).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(key: &str, name: &str, path: &Path) -> Target {
        Target { key: key.into(), name: name.into(), path: path.to_path_buf() }
    }

    #[test]
    fn default_config_roundtrips_through_toml() {
        let c = Config::default();
        let text = to_toml(&c);
        assert!(text.starts_with("# Lumiwake"));
        assert_eq!(parse(&text, Path::new("x")).unwrap(), c);
    }

    #[test]
    fn parses_hand_written_toml_and_normalizes_keys() {
        let text = r#"
            [general]
            delete_folder = ""
            on_exit = "delete"

            [keys]
            undo = "z+ctrl"

            [[targets]]
            key = "shift+ctrl+1"
            path = "/photos/風景"
        "#;
        let c = parse(text, Path::new("x")).unwrap();
        assert_eq!(c.general.delete_folder, None, "空文字は未指定");
        assert_eq!(c.general.on_exit, OnExit::Delete);
        assert_eq!(c.keys.skip, "Space", "書いていない項目は既定値");
        assert_eq!(c.keys.undo, "Ctrl+Z");
        assert_eq!(c.targets[0].key, "Ctrl+Shift+1");
        assert_eq!(c.targets[0].display_name(), "風景");
    }

    #[test]
    fn old_files_are_migrated_to_the_new_toggle_key() {
        // v0（version なし）で既定のまま F になっている
        let c = parse("[keys]\ntoggle_view = \"F\"\n", Path::new("x")).unwrap();
        assert_eq!(c.version, CONFIG_VERSION);
        assert_eq!(c.keys.toggle_view, "Ctrl+Shift+F");
        assert_eq!(c.keys.toggle_paths, "Ctrl+Shift+P", "新しい項目は既定値");
        assert!(c.general.show_paths);

        // 自分で変えていたキーはそのまま
        let c = parse("[keys]\ntoggle_view = \"Alt+V\"\n", Path::new("x")).unwrap();
        assert_eq!(c.keys.toggle_view, "Alt+V");

        // 最新の形式で F を選んでいる場合は変えない
        let c = parse("version = 1\n[keys]\ntoggle_view = \"F\"\n", Path::new("x")).unwrap();
        assert_eq!(c.keys.toggle_view, "F");
    }

    #[test]
    fn syntax_errors_are_reported() {
        assert!(matches!(parse("[general\n", Path::new("x")), Err(ConfigError::Parse { .. })));
        assert!(matches!(parse("[general]\non_exit = \"never\"", Path::new("x")), Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn detects_duplicate_and_reserved_keys() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config {
            targets: vec![
                target("1", "風景", dir.path()),
                target("1", "人物", dir.path()),
                target("Space", "後で", dir.path()),
                target("F5", "再読込", dir.path()),
                target("Ctrl+?", "変", dir.path()),
            ],
            ..Default::default()
        };
        let issues = c.validate();
        let errors: Vec<_> = issues.iter().filter(|i| i.severity == Severity::Error).collect();
        assert!(errors.iter().any(|i| i.key.as_deref() == Some("1")));
        assert!(errors.iter().any(|i| i.key.as_deref() == Some("Space")), "操作キーとの重複も検出");
        assert!(errors.iter().any(|i| i.message.contains("Ctrl+?")));
        assert!(issues.iter().any(|i| i.severity == Severity::Warning && i.key.as_deref() == Some("F5")));
    }

    #[test]
    fn detects_path_problems() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.general.delete_folder = Some(dir.path().to_path_buf());
        c.targets = vec![
            target("1", "削除と同じ", dir.path()),
            target("2", "ない", &dir.path().join("missing")),
            target("3", "空", Path::new("")),
        ];
        let issues = c.validate();
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("削除フォルダと同じ")));
        assert!(issues.iter().any(|i| i.severity == Severity::Warning && i.message.contains("見つかりません")));
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("指定されていません")));
    }

    #[test]
    fn load_creates_default_file_and_save_writes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let mut c = load_or_create(&path).unwrap();
        assert_eq!(c, Config::default());
        assert!(path.exists());

        c.targets.push(target("ctrl+2", "人物", dir.path()));
        let (saved, _) = save(&path, &c).unwrap();
        assert_eq!(saved.targets[0].key, "Ctrl+2");
        assert_eq!(load_or_create(&path).unwrap(), saved);
    }

    #[test]
    fn update_file_keeps_hand_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let in_memory = load_or_create(&path).unwrap();
        // アプリの起動中に手で編集された
        let mut edited = in_memory.clone();
        edited.targets.push(target("5", "手で追加", dir.path()));
        std::fs::write(&path, to_toml(&edited)).unwrap();

        let updated = update_file(&path, |c| c.general.view_mode = ViewMode::Focus).unwrap();
        assert_eq!(updated.general.view_mode, ViewMode::Focus);
        assert_eq!(updated.targets.len(), 1, "手で追加した振り分け先が残る");
        assert_eq!(load_or_create(&path).unwrap(), updated);
    }

    #[test]
    fn update_file_does_not_touch_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[general\n").unwrap();
        assert!(update_file(&path, |c| c.general.view_mode = ViewMode::Focus).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[general\n");
    }

    #[test]
    fn save_refuses_invalid_config_and_keeps_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = load_or_create(&path).unwrap();
        let mut c = before.clone();
        c.keys.skip = "Delete".into(); // 削除キーと重複
        assert!(matches!(save(&path, &c), Err(ConfigError::Invalid(_))));
        assert_eq!(load_or_create(&path).unwrap(), before);
    }
}
