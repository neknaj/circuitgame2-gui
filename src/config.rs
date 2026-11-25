//! ブラウザーが選択したファイルから使う、実行設定とソースパス。

use std::{collections::BTreeSet, ops::Range};

use circuitgame2_core::project::{SourceKind, SourceSnapshot, normalize_path};
pub(crate) use circuitgame2_project_config::Vram;
use circuitgame2_project_config::{ConfigError, FILE_NAME as SETTINGS, Spanned, parse};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::{CompileError, Simulation};

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
        let config = parse(bytes).map_err(|error| match error {
            ConfigError::InvalidUtf8(span) => at(Some(span), "不正な UTF-8 が含まれる。"),
            ConfigError::InvalidToml { span, reason } => {
                at(span, &format!("設定を解釈できない。{reason}"))
            }
            ConfigError::EmptySources(span) => {
                at(Some(span), "sources にソースを1件以上指定する。")
            }
        })?;
        if let Some(device) = config.device.first() {
            return Err(at(
                Some(device.span()),
                "この GUI は外部デバイスの接続に未対応。",
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
        self.compile_texts(&texts)
            .map_err(|error| error.to_string())
    }
}

impl ProjectSettings {
    /// `source_paths()` と同じ順序の本文を借用してコンパイルする。
    ///
    /// # Errors
    ///
    /// 本文の件数、ソース指定、開始モジュール、回路の展開または VRAM 設定に問題がある場合に失敗する。
    pub fn compile_texts(&self, texts: &[String]) -> Result<Simulation, CompileError> {
        let snapshots = self.snapshots(texts)?;
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

    /// 設定とソースを使い、デスクトップ GPU 向けの順序 NOR 回路を組み立てる。
    ///
    /// # Errors
    ///
    /// 本文の件数、ソース指定、開始モジュール、または回路の展開に問題がある場合と、
    /// VRAM を設定した場合に失敗する。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compile_gpu_texts(
        &self,
        texts: &[String],
    ) -> Result<circuitgame2_core::netlist::ClockedNor, CompileError> {
        let snapshots = self.snapshots(texts)?;
        if let Some(vram) = self.vram.first() {
            return Err(at(
                Some(vram.read_output.span()),
                "GPU の順序 NOR 実行は VRAM に未対応。",
            )
            .into());
        }
        let entry = self
            .entry
            .as_ref()
            .map_or("::main", |entry| entry.get_ref());
        crate::gpu_source::compile_gpu_snapshots(
            &snapshots,
            entry,
            self.entry.as_ref().map(Spanned::span),
        )
    }

    fn snapshots<'a>(
        &'a self,
        texts: &'a [String],
    ) -> Result<Vec<SourceSnapshot<'a>>, CompileError> {
        if texts.len() != self.paths.len() {
            return Err(at(None, "sources に対応する本文の件数が一致しない。").into());
        }
        // 先に指定したファイルの取得に失敗した場合は、ブラウザーがその診断を先に出す。
        if let Some(error) = &self.path_failure {
            return Err(error.clone().into());
        }
        Ok(self
            .paths
            .iter()
            .zip(texts)
            .map(|((path, _), text)| SourceSnapshot {
                path,
                revision: 1,
                text,
            })
            .collect())
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
            "{raw:?} は NCG2M のソース。この GUI は NCG2M の実行に未対応。"
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
