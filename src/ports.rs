//! ルート回路のポート名を、VM が確定したビット範囲に対応付ける。

use std::ops::Range;

use circuitgame2_core::{
    netlist::PortBitLayout,
    project::{CircuitProject, ModuleBodyPosition},
    syntax::{
        Form,
        circuit::{PortSite, routine_ports},
    },
};

pub(crate) struct Port {
    name: String,
    bits: Range<usize>,
}

/// 宣言と VM のポート配列を対応付ける。名前の範囲またはポートの件数が合わなければ None を返す。
pub(crate) fn module_ports(
    project: &CircuitProject<'_>,
    position: ModuleBodyPosition,
    inputs: &PortBitLayout,
    outputs: &PortBitLayout,
) -> Option<(Vec<Port>, Vec<Port>)> {
    let source = project.sources().get(position.file)?.text();
    let syntax = &project.circuits().get(position.file)?.syntax;
    let sites = routine_ports(source, syntax, position.entry)?;
    let collect = |sites: Vec<PortSite>, layout: &PortBitLayout| {
        if sites.len() != layout.ranges().len() {
            return None;
        }
        sites
            .into_iter()
            .zip(layout.ranges())
            .map(|(site, bits)| {
                let Form::Atom { span, .. } = syntax.forms.get(site.name)? else {
                    return None;
                };
                Some(Port {
                    name: source.get(span.clone())?.to_owned(),
                    bits: bits.clone(),
                })
            })
            .collect()
    };
    Some((
        collect(sites.inputs, inputs)?,
        collect(sites.outputs, outputs)?,
    ))
}

pub(crate) fn signal_name(ports: &[Port], bit: usize) -> Option<String> {
    let port = ports.get(ports.partition_point(|port| port.bits.end <= bit))?;
    if !port.bits.contains(&bit) {
        return None;
    }
    if port.bits.len() == 1 {
        return Some(port.name.clone());
    }
    Some(format!("{}[{}]", port.name, bit - port.bits.start))
}
