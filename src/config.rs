//! config.toml の読み込みと検証。

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::entry::{EntryType, Prefix};

pub const APP_ID: &str = "tsuratsura";

/// 初回起動時に生成する config（リポジトリの `config.example.toml`）。内蔵デフォルトもこれと同じ内容。
pub const DEFAULT_CONFIG_TOML: &str = include_str!("../config.example.toml");

const DEBOUNCE_RANGE: std::ops::RangeInclusive<u64> = 50..=10_000;
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<u16> = 8..=72;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// 空ならOS標準のアプリデータディレクトリ。指定する場合は絶対パスのみ。
    pub data_dir: String,
    /// 本文に使うインストール済みフォントのファミリー名。空なら同梱フォント。
    pub font_family: String,
    pub font_size: u16,
    /// 起動時にウィンドウを最前面へ固定する。
    pub always_on_top: bool,
    pub autosave_debounce_ms: u64,
    /// 日次メモを残す日数。0 なら削除しない。
    pub retention_days: u32,
    pub entry_header: String,
    pub entry_types: Vec<EntryType>,
    /// 空ならプレフィックスのセレクタを開かない。
    pub prefixes: Vec<Prefix>,
}

impl Default for Config {
    fn default() -> Self {
        let entry = |id: &str, label: &str, key: &str| EntryType {
            id: id.into(),
            label: label.into(),
            key: key.into(),
            template: String::new(),
        };
        let prefix = |text: &str, key: &str| Prefix {
            text: text.into(),
            key: key.into(),
        };

        Self {
            data_dir: String::new(),
            font_family: String::new(),
            font_size: 15,
            always_on_top: false,
            autosave_debounce_ms: 250,
            retention_days: 30,
            entry_header: "[{time}] {label}".into(),
            entry_types: vec![
                entry("work", "作業メモ", "w"),
                entry("redmine", "Redmine確認", "r"),
                entry("slack", "Slack確認", "s"),
                entry("mail", "メール", "m"),
                entry("meeting", "打ち合わせ", "g"),
            ],
            prefixes: vec![
                prefix("<action item> ", "a"),
                prefix("<remind> ", "r"),
                prefix("<重要> ", "i"),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Parse(String),
    Invalid(String),
    Io(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "configを解析できません: {e}"),
            Self::Invalid(e) => write!(f, "configが不正です: {e}"),
            Self::Io(e) => write!(f, "configを読み込めません: {e}"),
        }
    }
}

impl Config {
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(source).map_err(|e| ConfigError::Parse(e.message().to_owned()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err(ConfigError::Invalid(msg));

        if !DEBOUNCE_RANGE.contains(&self.autosave_debounce_ms) {
            return invalid(format!(
                "autosave_debounce_ms は {}〜{} の範囲で指定してください",
                DEBOUNCE_RANGE.start(),
                DEBOUNCE_RANGE.end()
            ));
        }
        if !FONT_SIZE_RANGE.contains(&self.font_size) {
            return invalid(format!(
                "font_size は {}〜{} の範囲で指定してください",
                FONT_SIZE_RANGE.start(),
                FONT_SIZE_RANGE.end()
            ));
        }
        if !self.data_dir.is_empty() && !Path::new(&self.data_dir).is_absolute() {
            return invalid("data_dir は空か絶対パスで指定してください".into());
        }
        if self.entry_types.is_empty() {
            return invalid("entry_types が空です".into());
        }

        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for entry in &self.entry_types {
            if entry.id.trim().is_empty() {
                return invalid("entry_types に空の id があります".into());
            }
            if !ids.insert(entry.id.as_str()) {
                return invalid(format!("id \"{}\" が重複しています", entry.id));
            }
            if entry.label.trim().is_empty() {
                return invalid(format!("id \"{}\" の label が空です", entry.id));
            }
            let Some(key) = entry.key_char() else {
                return invalid(format!("id \"{}\" の key は1文字にしてください", entry.id));
            };
            if !keys.insert(key) {
                return invalid(format!("key \"{}\" が重複しています", entry.key));
            }
        }

        let mut keys = HashSet::new();
        for prefix in &self.prefixes {
            if prefix.text.trim().is_empty() {
                return invalid("prefixes に空の text があります".into());
            }
            if prefix.text.contains(['\n', '\r']) {
                return invalid(format!(
                    "prefixes の text \"{}\" に改行は使えません",
                    prefix.text.escape_debug()
                ));
            }
            let Some(key) = prefix.key_char() else {
                return invalid(format!(
                    "prefixes の text \"{}\" の key は1文字にしてください",
                    prefix.text
                ));
            };
            if !keys.insert(key) {
                return invalid(format!(
                    "prefixes の key \"{}\" が重複しています",
                    prefix.key
                ));
            }
        }

        Ok(())
    }

    /// 日次ファイルを置くディレクトリ。
    pub fn resolve_data_dir(&self, default_data_dir: &Path) -> PathBuf {
        if self.data_dir.is_empty() {
            default_data_dir.to_path_buf()
        } else {
            PathBuf::from(&self.data_dir)
        }
    }
}

/// config を読み込む。存在しなければ初期configを生成する。
///
/// エラー時はユーザーconfigに触れず、内蔵デフォルトとエラーを返す。
pub fn load(path: &Path) -> (Config, Option<ConfigError>) {
    match fs::read_to_string(path) {
        Ok(source) => match Config::parse(&source) {
            Ok(config) => (config, None),
            Err(e) => (fallback_keeping_data_dir(&source), Some(e)),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let error = write_initial(path)
                .err()
                .map(|e| ConfigError::Io(format!("初期configを作成できません: {e}")));
            (Config::default(), error)
        }
        Err(e) => (Config::default(), Some(ConfigError::Io(e.to_string()))),
    }
}

/// config が不正でも、有効な `data_dir` だけは引き継ぐ。
/// 別の場所で今日のメモを開いて記録が分かれるのを防ぐ。
fn fallback_keeping_data_dir(source: &str) -> Config {
    let mut config = Config::default();
    let data_dir = toml::from_str::<toml::Table>(source)
        .ok()
        .and_then(|table| table.get("data_dir")?.as_str().map(str::to_owned))
        .filter(|dir| Path::new(dir).is_absolute());
    if let Some(dir) = data_dir {
        config.data_dir = dir;
    }
    config
}

fn write_initial(path: &Path) -> io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(DEFAULT_CONFIG_TOML.as_bytes())
}

/// `(configファイル, デフォルトのデータディレクトリ)`。OS標準の場所が取れなければカレントディレクトリ配下。
pub fn paths() -> (PathBuf, PathBuf) {
    default_paths().unwrap_or_else(|| {
        let fallback = std::env::current_dir().unwrap_or_default().join(APP_ID);
        (fallback.join("config.toml"), fallback)
    })
}

/// OS標準の `(configファイル, デフォルトのデータディレクトリ)`。
pub fn default_paths() -> Option<(PathBuf, PathBuf)> {
    let dirs = directories::BaseDirs::new()?;
    Some((
        dirs.config_dir().join(APP_ID).join("config.toml"),
        dirs.data_dir().join(APP_ID),
    ))
}
