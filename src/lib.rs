//! Core の順序回路 VM を実行し、WASM ではブラウザから呼べる形にする。

mod config;
mod diagnostic;
#[cfg(not(target_arch = "wasm32"))]
mod gpu_source;

use std::{collections::BTreeSet, ops::Range};

use circuitgame2_core::{
    project::{
        CircuitProjectError, ClockedModuleVm, ClockedModuleVmLimits, ModuleBodyPosition, NameKey,
        PreparedDefinitionError, SourceSnapshot, UseAliases, VramDeclaration, VramOutputError,
        VramSetupError, parse_circuit_project, prepare_definition_analysis,
    },
    syntax::circuit::Head,
};
pub use circuitgame2_project_config::FILE_NAME as PROJECT_CONFIG_FILE_NAME;
pub use config::{ProjectSettings, decode_utf8};
pub use diagnostic::{CompileError, SourceLocation};
#[cfg(not(target_arch = "wasm32"))]
pub use gpu_source::compile_gpu_source;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

// CLI の例題と同じ範囲で回路を実行する。
const LIMITS: ClockedModuleVmLimits = ClockedModuleVmLimits {
    width: 65_536,
    wiring_bits: 1_048_576,
    gates: 1_048_576,
    state_bits: 1_048_576,
};

/// ブラウザーでリセットと tick 操作を行える順序回路。
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Simulation {
    compiled: ClockedModuleVm,
    vram: Vec<(String, usize)>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Simulation {
    /// UTF-8 の単独ソースから、ルートモジュール `::main` をコンパイルする。
    ///
    /// # Errors
    ///
    /// ソースの構文や宣言、配線、型、資源上限に問題がある場合はエラーを返す。
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(source: &str) -> Result<Simulation, String> {
        Self::compile(source).map_err(|error| error.to_string())
    }

    /// tick ごとに渡す入力のビット数を返す。
    pub fn input_width(&self) -> usize {
        self.compiled.inputs.total_bits()
    }

    /// 各 tick の回路出力に含まれるビット数を返す。
    pub fn output_width(&self) -> usize {
        self.compiled.outputs.total_bits()
    }

    /// RAM の公開名を設定の初出順で返す。
    pub fn vram_names(&self) -> Vec<String> {
        self.vram.iter().map(|(name, _)| name.clone()).collect()
    }

    /// 公開名が指す RAM の記憶容量をビット単位で返す。
    ///
    /// 名前が見つからない場合は `None` を返す。
    pub fn vram_bit_count(&self, name: &str) -> Option<usize> {
        self.vram_memory(name).map(<[bool]>::len)
    }

    /// 確定済みの RAM から `offset` 以降を最大 `count` ビット読み取る。
    ///
    /// 語はアドレス順、各語は最下位ビットから並ぶ。名前が見つからない場合や
    /// `offset` が記憶部の末尾を超える場合は `None` を返す。
    pub fn vram_page(&self, name: &str, offset: usize, count: usize) -> Option<String> {
        let memory = self.vram_memory(name)?.get(offset..)?;
        let len = memory.len().min(count);
        Some(bit_text(memory[..len].iter().copied()))
    }

    /// 回路を初期状態へ戻し、tick 0 の出力を返す。
    pub fn reset(&mut self) -> String {
        bit_text(self.compiled.vm.reset())
    }

    /// 入力を適用して 1 tick 進め、確定した回路出力を返す。
    ///
    /// 入出力はポートの宣言順で、各ポートの最下位ビットから並べる。
    ///
    /// # Errors
    ///
    /// 入力の幅や文字が不正な場合は、回路を進めずエラーを返す。
    pub fn tick(&mut self, input: &str) -> Result<String, String> {
        if input.len() != self.input_width()
            || !input.bytes().all(|byte| matches!(byte, b'0' | b'1'))
        {
            return Err(format!(
                "入力を {} 個の 0 または 1 で指定する。",
                self.input_width()
            ));
        }
        let input: Vec<_> = input.bytes().map(|byte| byte == b'1').collect();
        self.compiled
            .vm
            .step(&input)
            .map(bit_text)
            .map_err(|error| format!("tick を実行できない。 {error:?}"))
    }
}

impl Simulation {
    /// UTF-8 の単独ソースから `::main` をコンパイルし、構文診断の位置を保持する。
    ///
    /// # Errors
    ///
    /// ソースの構文や宣言、配線、型、資源上限に問題がある場合にエラーを返す。
    pub fn compile(source: &str) -> Result<Self, CompileError> {
        let snapshots = [SourceSnapshot {
            path: "main.ncg2",
            revision: 1,
            text: source,
        }];
        Self::from_snapshots(&snapshots, "::main", None, &[])
    }

    fn vram_memory(&self, name: &str) -> Option<&[bool]> {
        let (_, index) = self.vram.iter().find(|(candidate, _)| candidate == name)?;
        self.compiled.vm.ram_memory(*index)
    }

    fn from_snapshots(
        snapshots: &[SourceSnapshot<'_>],
        name: &str,
        name_span: Option<Range<usize>>,
        vram: &[config::Vram],
    ) -> Result<Self, CompileError> {
        with_module_entry(snapshots, name, name_span.clone(), |aliases, position| {
            let declarations: Vec<_> = vram
                .iter()
                .map(|item| VramDeclaration {
                    name: &item.name,
                    read_output: item.read_output.get_ref(),
                })
                .collect();
            let compiled = aliases
                .lower_clocked_vram(position, LIMITS, &declarations)
                .map_err(|error| match error {
                    VramSetupError::Circuit(error) => {
                        format!("{name} を展開できない ({error:?})。")
                    }
                    VramSetupError::Output { declaration, error } => {
                        let span = vram.get(declaration).map(|item| item.read_output.span());
                        config::at(span, &vram_error(error))
                    }
                    VramSetupError::Allocation => {
                        "VRAM の公開設定を保持する領域を確保できない。".into()
                    }
                })?
                .ok_or_else(|| entry_message(&name_span, format!("{name} を実行できない。")))?;
            let mut names = BTreeSet::new();
            let mut bindings = Vec::new();
            for declaration in &declarations {
                if names.insert(declaration.name) {
                    let index = compiled
                        .vram_storage_index(declaration.name)
                        .ok_or_else(|| {
                            config::at(
                                None,
                                &format!(
                                    "公開 VRAM {:?} の記憶部が見つからない。",
                                    declaration.name
                                ),
                            )
                        })?;
                    bindings.push((declaration.name.to_owned(), index));
                }
            }
            Ok(Self {
                compiled: compiled.into_module(),
                vram: bindings,
            })
        })
    }
}

fn entry_message(span: &Option<Range<usize>>, message: String) -> String {
    match span {
        Some(span) => config::at(Some(span.clone()), &message),
        None => message,
    }
}

// VM と GPU の変換が、同じスナップショット上で検査した宣言と別名を使う。
fn with_module_entry<T>(
    snapshots: &[SourceSnapshot<'_>],
    name: &str,
    name_span: Option<Range<usize>>,
    compile: impl FnOnce(&UseAliases<'_, '_, '_>, ModuleBodyPosition) -> Result<T, CompileError>,
) -> Result<T, CompileError> {
    let project = parse_circuit_project(snapshots).map_err(|error| match error {
        CircuitProjectError::Syntax { id, error, .. } => CompileError::Syntax {
            path: id.components().join("/"),
            error,
        },
        other => CompileError::Message(format!("ソースを読み込めない ({other:?})。")),
    })?;
    let prepared = prepare_definition_analysis(&project)
        .map_err(|error| format!("宣言を検査できない。 {error:?}"))?;
    let analysis = prepared.resolve().map_err(|error| match error {
        PreparedDefinitionError::Alias(error) => format!("宣言を検査できない。 {error:?}"),
        PreparedDefinitionError::Definition { error, .. } => {
            format!("定義内の参照を解決できない。 {error:?}")
        }
    })?;
    let at_entry = |message: String| entry_message(&name_span, message);
    let qualified = name
        .strip_prefix("::")
        .filter(|path| {
            path.split("::")
                .all(|part| !part.is_empty() && !part.contains(':'))
        })
        .ok_or_else(|| at_entry("開始モジュールを完全修飾名で指定する。".into()))?;
    let (parent, unqualified) = qualified
        .rsplit_once("::")
        .map_or((None, qualified), |(parent, tail)| (Some(parent), tail));
    let key = NameKey {
        parent,
        name: unqualified,
    };
    let (declaration_source, declaration) = prepared
        .targets()
        .exports()
        .declarations()
        .declaration(key)
        .ok_or_else(|| at_entry(format!("{name} の宣言がない。")))?;
    if declaration.head != Head::Module {
        return Err(at_entry(format!("{name} は module を指す必要がある。")).into());
    }
    let file = project
        .sources()
        .iter()
        .position(|source| source.id.same_file(&declaration_source.id))
        .ok_or_else(|| at_entry(format!("{name} の宣言元ファイルが見つからない。")))?;
    let entry = project
        .circuits()
        .get(file)
        .and_then(|parsed| {
            parsed
                .syntax
                .entries
                .iter()
                .position(|entry| entry.form == declaration.form)
        })
        .ok_or_else(|| at_entry(format!("{name} の宣言位置が見つからない。")))?;
    let position = ModuleBodyPosition { file, entry };
    compile(analysis.aliases(), position)
}

/// 設定側の主範囲に加え、読み出し信号が指した定義の範囲を示す。
fn vram_error(error: VramOutputError<'_, '_>) -> String {
    match error {
        VramOutputError::MissingDefinition => {
            "read-output が指定する信号の定義が見つからない。".into()
        }
        VramOutputError::NotDirectMemory(site) => format!(
            "read-output にはルート本体で RAM を直接呼んだ定義の出力名を指定する。\n回路側の定義 {} [{}..{}]",
            site.source.id.components().join("/"),
            site.span.start,
            site.span.end
        ),
        VramOutputError::Module(_) => "VRAM の対象モジュールを検査できない。".into(),
    }
}

fn bit_text(bits: impl IntoIterator<Item = bool>) -> String {
    bits.into_iter()
        .map(|bit| if bit { '1' } else { '0' })
        .collect()
}
