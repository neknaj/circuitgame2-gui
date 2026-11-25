#![cfg(not(target_arch = "wasm32"))]

use circuitgame2_core::netlist::{NorGate, NorNetlist, Wire};
use circuitgame2_gpu::{GpuCombinationalNor32, GpuNorError};

// 実機のテストは一度に一つだけ GPU デバイスを作成する。
static GPU_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
#[ignore = "Vulkan GPU が必要"]
fn combinational_dispatch_matches_core_across_workgroups() {
    let _device = GPU_TEST.lock().unwrap();
    const WORDS: usize = 129;
    const GATES: usize = 67;
    let netlist = NorNetlist {
        input_count: 2,
        gates: (0..GATES)
            .map(|gate| NorGate {
                left: if gate == 0 {
                    Wire::Input(0)
                } else {
                    Wire::Gate(gate - 1)
                },
                right: if gate % 3 == 0 {
                    Wire::Constant(false)
                } else {
                    Wire::Input(1)
                },
            })
            .collect(),
        outputs: vec![
            Wire::Gate(GATES - 1),
            Wire::Input(1),
            Wire::Constant(true),
            Wire::Gate(0),
            Wire::Constant(false),
        ],
    };
    let mut gpu =
        GpuCombinationalNor32::new(netlist.packed_combinational_layout32().unwrap()).unwrap();
    let inputs: Vec<_> = (0..2 * WORDS)
        .map(|index| (index as u32).wrapping_mul(0xc2b2_ae35).rotate_right(7))
        .collect();
    for words in [WORDS, 1] {
        let packed: Vec<_> = inputs
            .chunks_exact(WORDS)
            .flat_map(|port| port[..words].iter().copied())
            .collect();
        let output = gpu.evaluate(words as u32, &packed).unwrap();
        let mut expected = vec![0; words * netlist.outputs.len()];
        for word in 0..words {
            for lane in 0..u32::BITS {
                let input = [
                    packed[word] & (1 << lane) != 0,
                    packed[words + word] & (1 << lane) != 0,
                ];
                for (port, value) in netlist.evaluate(&input).unwrap().into_iter().enumerate() {
                    expected[port * words + word] |= u32::from(value) << lane;
                }
            }
        }
        assert_eq!(output, expected);
    }
    assert!(matches!(
        gpu.evaluate(1, &[0]),
        Err(GpuNorError::InputLength)
    ));
    assert!(matches!(
        gpu.check_batch(u32::MAX),
        Err(GpuNorError::ResourceLimit)
    ));
    println!("{:?}", gpu.adapter_info());
}

#[test]
#[ignore = "Vulkan GPU が必要"]
fn combinational_dispatch_handles_empty_ports_and_gates() {
    let _device = GPU_TEST.lock().unwrap();
    for (inputs, outputs, packed, expected) in [
        (
            0,
            vec![Wire::Constant(true), Wire::Constant(false)],
            vec![],
            vec![u32::MAX, 0],
        ),
        (1, vec![], vec![9], vec![]),
        (0, vec![], vec![], vec![]),
    ] {
        let netlist = NorNetlist {
            input_count: inputs,
            gates: vec![],
            outputs,
        };
        let mut gpu =
            GpuCombinationalNor32::new(netlist.packed_combinational_layout32().unwrap()).unwrap();
        assert_eq!(gpu.evaluate(1, &packed).unwrap(), expected);
        assert!(matches!(
            gpu.evaluate(0, &[]),
            Err(GpuNorError::ResourceLimit)
        ));
    }
}
