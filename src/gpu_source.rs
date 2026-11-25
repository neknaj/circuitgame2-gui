//! デスクトップ GPU 向けの組み合わせ回路と順序回路をソースから組み立てる。

use std::ops::Range;

use circuitgame2_core::{
    netlist::{ClockedNor, PackedCombinationalLayout32},
    project::{ClockedModuleError, LocatedName, ModuleBodyPosition, SourceSnapshot, UseAliases},
    syntax::circuit::Head,
};

use crate::{CompileError, LIMITS, SourceLocation, entry_message, with_circuit_entry, with_module_entry};

/// 開始宣言の意味論に対応する GPU の実行対象。
pub enum GpuBatchCircuit {
    /// 各入力を独立に評価し、公開出力を得る。
    Combinational(PackedCombinationalLayout32),
    /// 指定した入力列で tick を進め、各時点の公開出力を得る。
    Clocked(ClockedNor),
}

/// 単独ソースの `::main` を、宣言種別に対応する GPU の実行対象に変換する。
///
/// # Errors
///
/// ソース、開始宣言、配線、型、資源上限を検査できない場合にエラーを返す。
pub fn compile_gpu_batch_source(source: &str) -> Result<GpuBatchCircuit, CompileError> {
    let snapshots = [SourceSnapshot { path: "main.ncg2", revision: 1, text: source }];
    compile_gpu_batch_snapshots(&snapshots, "::main", None)
}

pub(crate) fn compile_gpu_batch_snapshots(
    snapshots: &[SourceSnapshot<'_>],
    name: &str,
    name_span: Option<Range<usize>>,
) -> Result<GpuBatchCircuit, CompileError> {
    with_circuit_entry(snapshots, name, &name_span, |aliases, position, head| {
        let unavailable = || entry_message(&name_span, format!("{name} を実行できない。"));
        match head {
            Head::Module => Ok(GpuBatchCircuit::Clocked(
                lower_clocked(aliases, position, name)?.ok_or_else(unavailable)?,
            )),
            Head::Func => {
                let circuit = aliases
                    .expand_reachable_function(
                        (position.file, position.entry), LIMITS.width, LIMITS.wiring_bits, LIMITS.gates,
                    )
                    .map_err(|error| entry_message(&name_span, format!("{name} の関数を展開できない。{error:?}")))?
                    .ok_or_else(unavailable)?;
                let layout = circuit.netlist().packed_combinational_layout32()
                    .map_err(|error| entry_message(&name_span, format!("{name} の配線を検査できない。{error:?}")))?;
                Ok(GpuBatchCircuit::Combinational(layout))
            }
            _ => Err(entry_message(&name_span, format!("{name} は func または module を指す必要がある。")).into()),
        }
    })
}

/// UTF-8 の単独ソースから開始モジュール `::main` を順序 NOR 回路に変換する。
///
/// # Errors
///
/// ソースを解析できない場合、開始モジュールを解決できない場合、変換対象外の式や素子を
/// 含む場合、または配線や資源の上限に違反する場合にエラーを返す。
pub fn compile_gpu_source(source: &str) -> Result<ClockedNor, CompileError> {
    let snapshots = [SourceSnapshot {
        path: "main.ncg2",
        revision: 1,
        text: source,
    }];
    compile_gpu_snapshots(&snapshots, "::main", None)
}

pub(crate) fn compile_gpu_snapshots(
    snapshots: &[SourceSnapshot<'_>],
    name: &str,
    name_span: Option<Range<usize>>,
) -> Result<ClockedNor, CompileError> {
    with_module_entry(snapshots, name, name_span.clone(), |aliases, position| {
        lower_clocked(aliases, position, name)?
            .ok_or_else(|| entry_message(&name_span, format!("{name} を実行できない。")).into())
    })
}

fn lower_clocked(
    aliases: &UseAliases<'_, '_, '_>, position: ModuleBodyPosition, name: &str,
) -> Result<Option<ClockedNor>, CompileError> {
    aliases.lower_clocked_module_with_limits(position, LIMITS.width, LIMITS.wiring_bits, LIMITS.gates)
        .map_err(|error| clocked_error(name, error))
}

// Core の位置を所有し、ワーカー終了後も同じスナップショットの範囲を使えるようにする。
fn location(site: &LocatedName<'_, '_>) -> SourceLocation {
    SourceLocation {
        path: site.source.id.components().join("/"),
        revision: site.source.revision,
        span: site.span.clone(),
    }
}

fn reported(
    site: &LocatedName<'_, '_>,
    related: impl IntoIterator<Item = SourceLocation>,
    message: impl Into<String>,
) -> CompileError {
    CompileError::Source {
        primary: location(site),
        related: related.into_iter().collect(),
        message: message.into(),
    }
}

fn chain<'s, 'p: 's, 'a: 'p>(
    sites: impl IntoIterator<Item = &'s LocatedName<'p, 'a>>,
    message: &str,
) -> CompileError {
    let mut sites = sites.into_iter();
    match sites.next() {
        Some(first) => reported(first, sites.map(location), message),
        None => message.to_owned().into(),
    }
}

fn clocked_error(name: &str, error: ClockedModuleError<'_, '_>) -> CompileError {
    use circuitgame2_core::project::{
        ModuleCallInputsError, ModuleCallsError, ModuleOutputTypesError, PrimitiveModuleError,
        StructureLayoutError, StructurePlacementError, StructureTypeError,
    };
    match error {
        ClockedModuleError::Unsupported(site) => reported(
            &site,
            [],
            "GPU の順序 NOR 変換でこの型、式、または呼び出しを扱えない。",
        ),
        ClockedModuleError::Capacity(site) => reported(&site, [], "ゲート数の上限を超えた。"),
        ClockedModuleError::AliasCycle(sites) => chain(&sites, "遅延のない配線が循環している。"),
        ClockedModuleError::Recursion(sites) => {
            chain(&sites, "モジュールの呼び出しが循環している。")
        }
        ClockedModuleError::StructureLayout(StructureTypeError::Cycle { references }) => chain(
            references.iter().map(|edge| &edge.location),
            "構造体のフィールド型が循環している。",
        ),
        ClockedModuleError::StructureLayout(StructureTypeError::Layout(
            StructureLayoutError::Placement { error, .. },
        )) => match error {
            StructurePlacementError::DuplicateName { later, prior } => reported(
                &later,
                [location(&prior)],
                "フィールド名を重複して宣言した。",
            ),
            StructurePlacementError::Overlap { later, prior, .. } => reported(
                &later,
                [location(&prior)],
                "フィールドのビット範囲が重なる。",
            ),
            StructurePlacementError::Outside {
                offset,
                start,
                field_width,
                structure_width,
            } => reported(
                &offset,
                [],
                format!(
                    "開始位置 {start}、幅 {field_width} のフィールドは構造体の幅 {structure_width} に収まらない。"
                ),
            ),
        },
        ClockedModuleError::StructureLayout(_) => {
            format!("{name} が使う構造体の型や幅を解決できない。").into()
        }
        ClockedModuleError::Parent(ModuleCallsError::Module(
            ModuleOutputTypesError::Mismatch {
                index,
                expected,
                actual,
            },
        )) => reported(
            &expected.origin,
            [location(&actual.origin)],
            format!("出力 {} の型が駆動元の型と一致しない。", index + 1),
        ),
        ClockedModuleError::Parent(ModuleCallsError::Call {
            reason:
                ModuleCallInputsError::Argument {
                    index,
                    argument,
                    required_at,
                    ..
                },
            ..
        }) => reported(
            &argument,
            required_at.iter().map(location),
            format!(
                "引数 {} が入力ポートの型や幅の条件を満たさない。",
                index + 1
            ),
        ),
        ClockedModuleError::Parent(_) => {
            format!("{name} の型、式、または結線を検査できない。").into()
        }
        ClockedModuleError::Child { call, reason } => {
            let related = match reason {
                PrimitiveModuleError::Port(site)
                | PrimitiveModuleError::Definition(site)
                | PrimitiveModuleError::Argument(site) => Some(location(&site)),
                PrimitiveModuleError::Module(_) => None,
            };
            reported(
                &call,
                related,
                "子モジュールを順序 NOR 回路に変換できない。",
            )
        }
        ClockedModuleError::Function { call, .. } => {
            reported(&call, [], "呼び出した関数を展開できない。")
        }
        ClockedModuleError::PortLayout { site, layout, .. } => reported(
            &site,
            [],
            format!("ポートのビット範囲を割り当てられない。{layout:?}"),
        ),
        ClockedModuleError::Instance(site, _) => {
            reported(&site, [], "呼び出しの配線を組み立てられない。")
        }
        ClockedModuleError::Netlist(error) => {
            format!("順序 NOR 回路の配線を検査できない。{error:?}").into()
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::ProjectSettings;

    #[test]
    fn a_project_uses_the_configured_entry_and_source_order() {
        let config =
            ProjectSettings::new(b"sources = ['basis.ncg2', 'src/nor.ncg2']\nentry = '::invert'")
                .unwrap();
        let texts = [
            "(using nor)".to_owned(),
            "(module invert (in (a (bits 1))) (out (q (bits 1))) (def q (nor a a)))".to_owned(),
        ];
        let circuit = config.compile_gpu_texts(&texts).unwrap();
        let initial = circuit.reset();
        assert_eq!(initial.outputs, [false]);
        assert_eq!(
            circuit.step(&initial.gates, &[false]).unwrap().outputs,
            [true]
        );
    }

    #[test]
    fn an_unsupported_state_element_retains_the_project_source_location() {
        let src = "(using nor) (latch hold (width 1)) (module main (in (a (bits 1)) (en (bits 1))) (out (q (bits 1))) (def q (hold a en)))".to_owned();
        let config = ProjectSettings::new(b"sources = ['src/modules.ncg2']").unwrap();
        let error = config.compile_gpu_texts(&[src.clone()]).err().unwrap();
        let (path, span) = error.source_location().unwrap();
        assert_eq!(path, "src/modules.ncg2");
        assert_eq!(src.get(span.clone()), Some("(hold a en)"));
    }

    #[test]
    fn recursion_keeps_related_locations_in_other_files() {
        let config = ProjectSettings::new(b"sources = ['left.ncg2', 'right.ncg2']").unwrap();
        let left =
            "(using nor) (module main (in (a (bits 1))) (out (q (bits 1))) (def q (other a)))";
        let right = "(module other (in (a (bits 1))) (out (q (bits 1))) (def q (main a)))";
        let error = config
            .compile_gpu_texts(&[left.into(), right.into()])
            .err()
            .unwrap();
        let (path, span) = error.source_location().unwrap();
        let related = error.related_source_locations();
        assert_eq!(related.len(), 1);
        assert_ne!(path, related[0].path);
        assert_eq!(related[0].revision, 1);
        match path {
            "left.ncg2" => {
                assert_eq!(left.get(span.clone()), Some("(other a)"));
                assert_eq!(right.get(related[0].span.clone()), Some("(main a)"));
            }
            "right.ncg2" => {
                assert_eq!(right.get(span.clone()), Some("(main a)"));
                assert_eq!(left.get(related[0].span.clone()), Some("(other a)"));
            }
            _ => panic!("unexpected path {path}"),
        }
    }
}
