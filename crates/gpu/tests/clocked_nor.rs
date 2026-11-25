#![cfg(not(target_arch = "wasm32"))]
use circuitgame2_core::netlist::{ClockedNor, NorGate, NorNetlist, Wire};
use circuitgame2_gpu::{GpuNor32, GpuNorError};

// 実機のテストは一度に一つだけ GPU デバイスを作成する。
static GPU_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
#[ignore = "Vulkan GPU が必要"]
fn native_gpu_matches_core_with_feedback_inputs_and_constants() {
    let _device = GPU_TEST.lock().unwrap();
    let clocked = ClockedNor::check(NorNetlist {
        input_count: 1,
        gates: vec![
            NorGate {
                left: Wire::Input(0),
                right: Wire::Constant(false),
            },
            NorGate {
                left: Wire::Gate(0),
                right: Wire::Gate(1),
            },
        ],
        outputs: vec![
            Wire::Gate(0),
            Wire::Gate(1),
            Wire::Input(0),
            Wire::Constant(false),
            Wire::Constant(true),
        ],
    })
    .unwrap();
    let mut gpu = GpuNor32::new(clocked.packed_layout32().unwrap()).unwrap();
    let initial = [1, 3, 33, 5, 4, 1 << 31];
    let stimuli = [0, 1, 300, 8, 91, 0, 1 << 31, 0, 0];
    let result = gpu.evaluate(3, 3, &initial, &stimuli).unwrap();
    let mut gates = [0; 6];
    let mut outputs = [0; 45];
    for word in 0..3 {
        let mut state = vec![u64::from(initial[word]), u64::from(initial[3 + word])];
        for tick in 0..3 {
            let result = clocked
                .step_packed(&state, &[u64::from(stimuli[tick * 3 + word])])
                .unwrap();
            for (port, value) in result.outputs.into_iter().enumerate() {
                outputs[(tick * 5 + port) * 3 + word] = value as u32;
            }
            state = result.gates;
        }
        for (gate, value) in state.into_iter().enumerate() {
            gates[gate * 3 + word] = value as u32;
        }
    }
    assert_eq!(result.outputs, outputs);
    assert_eq!(result.gates, gates);
    assert!(matches!(
        gpu.evaluate(3, 3, &[], &stimuli),
        Err(GpuNorError::InitialStateLength)
    ));
    assert!(matches!(
        gpu.evaluate(u32::MAX, 3, &initial, &stimuli),
        Err(GpuNorError::ResourceLimit)
    ));
}

#[test]
#[ignore = "Vulkan GPU が必要"]
fn submissions_preserve_tick_order_and_empty_buffers() {
    let _device = GPU_TEST.lock().unwrap();
    let layout = ClockedNor::check(NorNetlist {
        input_count: 1,
        gates: Vec::new(),
        outputs: vec![Wire::Input(0), Wire::Constant(true)],
    })
    .unwrap()
    .packed_layout32()
    .unwrap();
    let mut gpu = GpuNor32::new(layout).unwrap();
    let inputs: Vec<_> = (0..513).map(|tick| 1u32.rotate_left(tick)).collect();
    let trace = gpu.evaluate(1, 513, &[], &inputs).unwrap();
    assert!(trace.gates.is_empty());
    let expected: Vec<_> = inputs.iter().flat_map(|value| [*value, u32::MAX]).collect();
    assert_eq!(trace.outputs, expected);

    let layout = ClockedNor::check(NorNetlist {
        input_count: 0,
        gates: vec![NorGate {
            left: Wire::Gate(0),
            right: Wire::Constant(false),
        }],
        outputs: Vec::new(),
    })
    .unwrap()
    .packed_layout32()
    .unwrap();
    let mut gpu = GpuNor32::new(layout).unwrap();
    for (ticks, expected) in [(0, 1), (256, 1), (257, !1)] {
        let trace = gpu.evaluate(1, ticks, &[1], &[]).unwrap();
        assert_eq!(trace.gates, [expected]);
        assert!(trace.outputs.is_empty());
    }
}

#[test]
#[ignore = "Vulkan GPU が必要"]
fn dispatches_multiple_workgroups_per_tick() {
    let _device = GPU_TEST.lock().unwrap();
    const GATES: usize = 67;
    const WORDS: usize = 129;
    const TICKS: usize = 5;
    let clocked = ClockedNor::check(NorNetlist {
        input_count: 1,
        gates: (0..GATES)
            .map(|gate| NorGate {
                left: Wire::Gate((gate + 1) % GATES),
                right: Wire::Input(0),
            })
            .collect(),
        outputs: vec![Wire::Gate(0), Wire::Gate(GATES - 1)],
    })
    .unwrap();
    let mut gpu = GpuNor32::new(clocked.packed_layout32().unwrap()).unwrap();
    let initial: Vec<_> = (0..GATES * WORDS)
        .map(|index| (index as u32).wrapping_mul(0x9e37_79b9))
        .collect();
    let stimuli: Vec<_> = (0..TICKS * WORDS)
        .map(|index| (index as u32).wrapping_mul(0xc2b2_ae35))
        .collect();
    let trace = gpu
        .evaluate(WORDS as u32, TICKS as u32, &initial, &stimuli)
        .unwrap();
    for word in 0..WORDS {
        let mut state: Vec<_> = initial
            .chunks_exact(WORDS)
            .map(|row| u64::from(row[word]))
            .collect();
        for tick in 0..TICKS {
            let step = clocked
                .step_packed(&state, &[u64::from(stimuli[tick * WORDS + word])])
                .unwrap();
            for (port, output) in step.outputs.iter().enumerate() {
                assert_eq!(
                    trace.outputs[(tick * 2 + port) * WORDS + word],
                    *output as u32
                );
            }
            state = step.gates;
        }
        for (gate, value) in state.into_iter().enumerate() {
            assert_eq!(trace.gates[gate * WORDS + word], value as u32);
        }
    }
    println!("{:?}", gpu.adapter_info());
}
