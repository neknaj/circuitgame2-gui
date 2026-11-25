//! 回路のコンパイル結果から、編集画面が使う主範囲と表示文を得る。

use std::{error::Error, fmt, ops::Range};

use circuitgame2_core::syntax::circuit::CircuitFormError;

/// コンパイルで検査したソース上の位置。
#[derive(Debug)]
pub struct SourceLocation {
    /// プロジェクト内の正規化済み相対パス。
    pub path: String,
    /// コンパイルした本文に対応する文書版。
    pub revision: u64,
    /// UTF-8 の半開バイト範囲。
    pub span: Range<usize>,
}

/// 回路を実行する前に検出したエラー。
#[derive(Debug)]
pub enum CompileError {
    /// ソース上の位置を持つ字句、S 式、回路フォームの誤り。
    Syntax {
        /// プロジェクト内の正規化済み相対パス。
        path: String,
        /// コンパイルしたソースに対する Core の構文エラー。
        error: CircuitFormError,
    },
    /// 回路の展開で見つかった違反。
    Source {
        /// 違反を報告する主範囲。
        primary: SourceLocation,
        /// 同じ違反に関係するソース位置。
        related: Vec<SourceLocation>,
        /// 回路を展開できなかった理由。
        message: String,
    },
    /// 実行設定や回路の検査・展開で得た表示用のエラー。
    Message(String),
}

impl CompileError {
    /// 診断の主範囲に対応するファイルと半開バイト範囲を返す。
    ///
    /// この範囲を使う場合は、コンパイル時と同じ本文を参照する。
    pub fn source_location(&self) -> Option<(&str, &Range<usize>)> {
        match self {
            Self::Syntax { path, error } => {
                let span = match error {
                    CircuitFormError::Syntax(error) => &error.span,
                    CircuitFormError::Header { span, .. } => span,
                };
                Some((path, span))
            }
            Self::Source { primary, .. } => Some((&primary.path, &primary.span)),
            Self::Message(_) => None,
        }
    }

    /// 診断の主範囲に関係するソース位置を、診断が示す順に返す。
    pub fn related_source_locations(&self) -> &[SourceLocation] {
        match self {
            Self::Source { related, .. } => related,
            _ => &[],
        }
    }
}

impl From<String> for CompileError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax {
                path,
                error: CircuitFormError::Syntax(error),
            } => write!(
                formatter,
                "{path} [{}..{}] 構文エラー ({:?})。",
                error.span.start, error.span.end, error.kind,
            ),
            Self::Syntax {
                path,
                error: CircuitFormError::Header { kind, span },
            } => write!(
                formatter,
                "{path} [{}..{}] 回路フォームの構文エラー ({kind:?})。",
                span.start, span.end,
            ),
            Self::Source {
                primary, message, ..
            } => write!(
                formatter,
                "{} [{}..{}] {message}",
                primary.path, primary.span.start, primary.span.end
            ),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl Error for CompileError {}

#[cfg(test)]
mod tests {
    use crate::{CompileError, ProjectSettings, Simulation};

    #[test]
    fn syntax_location_survives_the_adapter_and_legacy_rendering() {
        let prefix = "; 回路🧪\r\n";
        let source = format!("{prefix})");
        let settings = ProjectSettings::new(b"sources = ['basis.ncg2', 'src/main.ncg2']").unwrap();
        let sources = vec!["(using nor)".into(), source.clone()];

        let result = settings.compile_texts(&sources);
        assert!(matches!(result, Err(CompileError::Syntax { .. })));
        let diagnostic = result.err().unwrap();
        assert_eq!(
            diagnostic.source_location(),
            Some(("src/main.ncg2", &(prefix.len()..source.len()))),
        );
        assert_eq!(
            settings.compile(sources).err().unwrap(),
            diagnostic.to_string(),
        );

        let single = Simulation::compile(&source).err().unwrap();
        assert_eq!(
            single.source_location(),
            Some(("main.ncg2", &(prefix.len()..source.len()))),
        );
        assert_eq!(Simulation::new(&source).err().unwrap(), single.to_string());
    }
}
