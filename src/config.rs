//! ブラウザーが選択したファイルから使う、実行設定とソースパス。

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
    ops::Range,
};

use circuitgame2_core::project::{SourceKind, SourceSnapshot, normalize_path};
use serde::Deserialize;
use toml::{Spanned, Table};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::Simulation;

const SETTINGS: &str = "circuitgame2.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Settings {
    sources: Spanned<Vec<Spanned<String>>>,
    entry: Option<Spanned<String>>,
    #[serde(rename = "tick-ns")]
    _tick_ns: Option<TickPeriod>,
    #[serde(default)]
    device: Vec<Spanned<Device>>,
    #[serde(default)]
    vram: Vec<Vram>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TickPeriod {
    #[serde(rename = "numerator")]
    _numerator: NonZeroU64,
    #[serde(rename = "denominator")]
    _denominator: NonZeroU64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "kind")]
    _kind: String,
    #[serde(rename = "bindings")]
    _bindings: BTreeMap<String, String>,
    #[serde(rename = "options")]
    _options: Option<Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Vram {
    pub(crate) name: String,
    pub(crate) read_output: Spanned<String>,
}

/// 設定ファイルの UTF-8 半開バイト範囲をエラーメッセージに付ける。
pub(crate) fn at(span: Option<Range<usize>>, reason: &str) -> String {
    match span {
        Some(span) => format!("{SETTINGS}[{}..{}] {reason}", span.start, span.end),
        None => format!("{SETTINGS} {reason}"),
    }
}

/// 設定ディレクトリを基準にしたソース一覧と開始モジュール。
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct ProjectSettings {
    paths: Vec<(String, Range<usize>)>,
    path_failure: Option<String>,
    entry: Option<Spanned<String>>,
    vram: Vec<Vram>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl ProjectSettings {
    /// TOML を読み、ソース指定を Core の仮想パスに正規化する。
    ///
    /// # Errors
    ///
    /// UTF-8 や設定の構文・項目が不正な場合、または外部デバイスを指定した場合に失敗する。
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(bytes: &[u8]) -> Result<ProjectSettings, String> {
        let text = decode_utf8(SETTINGS, bytes)?;
        let config: Settings = toml::from_str(&text).map_err(|error| {
            at(
                error.span(),
                &format!("設定を解釈できない。{}", error.message()),
            )
        })?;
        if config.sources.get_ref().is_empty() {
            return Err(at(
                Some(config.sources.span()),
                "sources にソースを1件以上指定する。",
            ));
        }
        if let Some(device) = config.device.first() {
            return Err(at(
                Some(device.span()),
                "この Web 版は外部デバイスの接続に未対応。",
            ));
        }
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        let mut path_failure = None;
        for value in config.sources.into_inner() {
            let path = match source_path(&value) {
                Ok(path) => path,
                Err(error) => {
                    path_failure = Some(error);
                    break;
                }
            };
            if seen.insert(path.clone()) {
                paths.push((path, value.span()));
            }
        }
        Ok(Self {
            paths,
            path_failure,
            entry: config.entry,
            vram: config.vram,
        })
    }

    /// ディレクトリから読むファイル名を設定の初出順で返す。
    ///
    /// 無効なパスがある場合は、その前に指定されたパスまでを返す。
    pub fn source_paths(&self) -> Vec<String> {
        self.paths.iter().map(|(path, _)| path.clone()).collect()
    }

    /// 設定内の元の指定位置を使い、取得できなかったファイルを示す。
    pub fn source_read_error(&self, index: usize) -> String {
        match self.paths.get(index) {
            Some((path, span)) => at(Some(span.clone()), &format!("{path:?} を読み込めない。")),
            None => at(None, "読み込めないファイルが設定の一覧にない。"),
        }
    }

    /// `source_paths()` と同じ順序の本文をコンパイルする。
    ///
    /// # Errors
    ///
    /// 本文の件数、ソース指定、開始モジュール、回路の展開または VRAM 設定に問題がある場合に失敗する。
    pub fn compile(&self, texts: Vec<String>) -> Result<Simulation, String> {
        if texts.len() != self.paths.len() {
            return Err(at(None, "sources に対応する本文の件数が一致しない。"));
        }
        // 先に指定したファイルの取得に失敗した場合は、ブラウザーがその診断を先に出す。
        if let Some(error) = &self.path_failure {
            return Err(error.clone());
        }
        let snapshots: Vec<_> = self
            .paths
            .iter()
            .zip(&texts)
            .map(|((path, _), text)| SourceSnapshot {
                path,
                revision: 1,
                text,
            })
            .collect();
        let entry = self
            .entry
            .as_ref()
            .map_or("::main", |entry| entry.get_ref().as_str());
        Simulation::from_snapshots(
            &snapshots,
            entry,
            self.entry.as_ref().map(Spanned::span),
            &self.vram,
        )
    }
}

fn source_path(value: &Spanned<String>) -> Result<String, String> {
    let at_value = |reason: &str| at(Some(value.span()), reason);
    let raw = value.get_ref();
    let path = normalize_path(raw)
        .map_err(|error| at_value(&format!("{raw:?} のパスを正規化できない ({error:?})。")))?;
    match path.source_kind() {
        Some(SourceKind::Circuit) => Ok(path.components().join("/")),
        Some(SourceKind::Functional) => Err(at_value(&format!(
            "{raw:?} は NCG2M のソース。この Web 版は NCG2M の実行に未対応。"
        ))),
        None => Err(at_value(&format!(
            "{raw:?} の拡張子には .ncg2 または .ncg2m を指定する。"
        ))),
    }
}

/// 不正な UTF-8 を置換せず、最初の不正なバイト範囲を報告する。
///
/// # Errors
///
/// UTF-8 に変換できないバイト列を受け取った場合にエラーを返す。
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn decode_utf8(path: &str, bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let start = error.valid_up_to();
        let end = error.error_len().map_or(bytes.len(), |width| start + width);
        format!("{path}[{start}..{end}] 不正な UTF-8 が含まれる。")
    })?;
    Ok(text.to_owned())
}
