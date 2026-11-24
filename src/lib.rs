//! ブラウザーとの入出力を変換し、Core の順序回路 VM を実行する。

use circuitgame2_core::{
    project::{
        CircuitProjectError, ClockedModuleVm, ClockedModuleVmLimits, ModuleBodyPosition, NameKey,
        SourceSnapshot, analyze_definitions, check_use_aliases, parse_circuit_project,
    },
    syntax::circuit::{CircuitFormError, Head},
};
use wasm_bindgen::prelude::*;

// CLI の例題と同じ範囲で回路を実行する。
const LIMITS: ClockedModuleVmLimits = ClockedModuleVmLimits {
    width: 65_536,
    wiring_bits: 1_048_576,
    gates: 1_048_576,
    state_bits: 1_048_576,
};

/// ブラウザーでリセットと tick 操作を行える順序回路。
#[wasm_bindgen]
pub struct Simulation {
    compiled: ClockedModuleVm,
}

#[wasm_bindgen]
impl Simulation {
    /// UTF-8 の単独ソースから、ルートモジュール `::main` をコンパイルする。
    ///
    /// # Errors
    ///
    /// ソースの構文や宣言、配線、型、資源上限に問題がある場合はエラーを返す。
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<Simulation, String> {
        let snapshots = [SourceSnapshot {
            path: "main.ncg2",
            revision: 1,
            text: source,
        }];
        let project = parse_circuit_project(&snapshots).map_err(source_error)?;
        let analysis = analyze_definitions(&project)
            .map_err(|error| format!("宣言を検査できない。 {error:?}"))?;
        let key = NameKey {
            parent: None,
            name: "main",
        };
        let (_, declaration) = analysis
            .aliases
            .targets
            .exports()
            .declarations()
            .declaration(key)
            .ok_or("::main の宣言がない。")?;
        if declaration.head != Head::Module {
            return Err("::main には module を指定する。".into());
        }
        let entry = project
            .circuits()
            .first()
            .and_then(|parsed| {
                parsed
                    .syntax
                    .entries
                    .iter()
                    .position(|entry| entry.form == declaration.form)
            })
            .ok_or("::main の構文上の位置を取得できない。")?;
        let aliases = check_use_aliases(&analysis.aliases.targets)
            .map_err(|error| format!("別名を解決できない ({error:?})。"))?;
        let compiled = aliases
            .lower_clocked_vm(ModuleBodyPosition { file: 0, entry }, LIMITS)
            .map_err(|error| format!("::main を展開できない。 {error:?}"))?
            .ok_or("::main を実行できない。")?;
        Ok(Self { compiled })
    }

    /// tick ごとに渡す入力のビット数を返す。
    pub fn input_width(&self) -> usize {
        self.compiled.inputs.total_bits()
    }

    /// 各 tick の回路出力に含まれるビット数を返す。
    pub fn output_width(&self) -> usize {
        self.compiled.outputs.total_bits()
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

/// 構文エラーの位置は元ソースの UTF-8 半開バイト範囲で示す。
fn source_error(error: CircuitProjectError<'_>) -> String {
    match error {
        CircuitProjectError::Syntax {
            error: CircuitFormError::Syntax(error),
            ..
        } => format!(
            "main.ncg2 [{}..{}] 構文エラー ({:?})。",
            error.span.start, error.span.end, error.kind
        ),
        CircuitProjectError::Syntax {
            error: CircuitFormError::Header { kind, span },
            ..
        } => format!(
            "main.ncg2 [{}..{}] 回路フォームの構文エラー ({kind:?})。",
            span.start, span.end
        ),
        other => format!("ソースを読み込めない ({other:?})。"),
    }
}

fn bit_text(bits: Vec<bool>) -> String {
    bits.into_iter()
        .map(|bit| if bit { '1' } else { '0' })
        .collect()
}
