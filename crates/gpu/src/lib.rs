//! デスクトップ GPU で組み合わせ NOR 回路と順序 NOR 回路を実行する。
#![cfg(not(target_arch = "wasm32"))]

mod combinational;

pub use combinational::GpuCombinationalNor32;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use circuitgame2_core::netlist::PackedNorLayout32;
use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 64;
const BINDINGS: u32 = 7;
const PARAMETERS: u32 = 5;
const TICKS_PER_SUBMIT: u32 = 256;
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);
const STORAGE: wgpu::BufferUsages = wgpu::BufferUsages::STORAGE;
const STATE: wgpu::BufferUsages = STORAGE.union(wgpu::BufferUsages::COPY_SRC);

/// ゲートの最終状態と各 tick の公開出力。
pub struct Trace {
    /// ゲート、レーンワードの順に並べた最終状態。
    pub gates: Vec<u32>,
    /// tick、公開ポート、レーンワードの順に並べた出力。
    pub outputs: Vec<u32>,
}

/// GPU 実行の失敗理由。
#[derive(Debug)]
pub enum GpuNorError {
    /// 使用できる Vulkan GPU が見つからない。
    AdapterUnavailable,
    /// デバイスとキューを取得できない。
    Device(wgpu::RequestDeviceError),
    /// デバイスが失われたか、処理が完了したことを確認できず再利用を中止した。
    DeviceUnavailable,
    /// GPU に投入した処理の完了を期限内に確認できなかった。
    Wait(wgpu::PollError),
    /// GPU の資源作成または計算コマンドが失敗した。
    Gpu(wgpu::Error),
    /// 初期状態の長さがゲート数とレーンワード数に合わない。
    InitialStateLength,
    /// 入力の長さがポート数とレーンワード数に合わない。順序評価では tick 数も照合する。
    InputLength,
    /// レーンワード数または GPU で使う添字や資源の大きさが許容範囲外である。
    ResourceLimit,
    /// GPU の処理完了または読み戻しを確認できなかった。
    Readback,
}

#[derive(Clone, Copy)]
struct Shape {
    lanes: u32,
    gates: u32,
    inputs: u32,
    ports: u32,
    state_words: u32,
    input_words: u32,
    trace_words: u32,
    trace_per_tick: u32,
    parameter_stride: u32,
    parameter_words: u32,
}

fn byte_size(words: u32) -> u64 {
    u64::from(words) * 4
}

impl Shape {
    fn check(
        layout: &PackedNorLayout32,
        lanes: u32,
        ticks: u32,
        limits: &wgpu::Limits,
    ) -> Option<Self> {
        let gates = u32::try_from(layout.gates().len()).ok()?;
        let inputs = u32::try_from(layout.input_count()).ok()?;
        let ports = u32::try_from(layout.outputs().len()).ok()?;
        u32::try_from(layout.source_count()).ok()?;
        let state_words = lanes.checked_mul(gates)?;
        let input_words = lanes.checked_mul(inputs)?.checked_mul(ticks)?;
        let trace_per_tick = lanes.checked_mul(ports)?;
        let trace_words = trace_per_tick.checked_mul(ticks)?;
        let wire_words = gates.checked_mul(2)?;
        let alignment = limits.min_storage_buffer_offset_alignment.max(4);
        let stride_alignment = u64::from(alignment).max(8);
        let parameter_stride =
            u32::try_from(byte_size(PARAMETERS).next_multiple_of(stride_alignment)).ok()?;
        let parameter_words = if state_words == 0 && trace_words == 0 {
            PARAMETERS
        } else {
            parameter_stride.checked_mul(ticks.saturating_sub(1))? / 4 + PARAMETERS
        };
        let storage_limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let staging_bytes = byte_size(state_words) + byte_size(trace_words);
        if lanes == 0
            || limits.max_bind_groups == 0
            || limits.max_bindings_per_bind_group < BINDINGS
            || limits.max_storage_buffers_per_shader_stage < BINDINGS
            || limits.max_buffers_and_acceleration_structures_per_shader_stage < BINDINGS
            || limits.max_non_sampler_bindings < BINDINGS
            || limits.max_dynamic_storage_buffers_per_pipeline_layout == 0
            || limits.max_compute_invocations_per_workgroup < WORKGROUP
            || limits.max_compute_workgroup_size_x < WORKGROUP
            || state_words.max(trace_per_tick).div_ceil(WORKGROUP)
                > limits.max_compute_workgroups_per_dimension
            || [
                state_words,
                input_words,
                trace_words,
                wire_words.max(2),
                ports,
                PARAMETERS,
            ]
            .into_iter()
            .any(|words| byte_size(words.max(1)) > storage_limit)
            || byte_size(parameter_words) > limits.max_buffer_size
            || staging_bytes > limits.max_buffer_size
        {
            return None;
        }
        Some(Self {
            lanes,
            gates,
            inputs,
            ports,
            state_words,
            input_words,
            trace_words,
            trace_per_tick,
            parameter_stride,
            parameter_words,
        })
    }
}

fn upload(device: &wgpu::Device, words: &[u32], empty: &[u32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(if words.is_empty() { empty } else { words }),
        usage: STORAGE,
    })
}

/// GPU に配線とパイプラインを保持し、同じ回路を繰り返し実行する。
///
/// Core の [順序 NOR 配線][PackedNorLayout32] を使う。1 ビットが 1 レーンを表し、
/// 各 tick のゲート更新は更新前の状態を読み、公開出力は更新後の状態を読む。
pub struct GpuNor32 {
    gpu: GpuDevice,
    layout: PackedNorLayout32,
    bindings: wgpu::BindGroupLayout,
    advance: wgpu::ComputePipeline,
    observe: wgpu::ComputePipeline,
    wiring: wgpu::Buffer,
    ports: wgpu::Buffer,
}

impl GpuNor32 {
    /// Vulkan GPU を選び、指定された回路を転送する。
    ///
    /// # Errors
    ///
    /// 対応 GPU の取得、回路の転送、またはパイプラインの作成に失敗した場合にエラーを返す。
    pub fn new(layout: PackedNorLayout32) -> Result<Self, GpuNorError> {
        let gpu = GpuDevice::new()?;
        let device = &gpu.device;
        Shape::check(&layout, 1, 0, &device.limits()).ok_or(GpuNorError::ResourceLimit)?;
        let (bindings, advance, observe, wiring, ports) = scoped(device, || {
            let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("NCG2 packed NOR"),
                entries: &(0..BINDINGS)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: binding != 1 && binding != 5,
                            },
                            has_dynamic_offset: binding == 6,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&bindings)],
                immediate_size: 0,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(include_str!("clocked_nor.wgsl").into()),
            });
            let [advance, observe] = ["advance", "observe"].map(|entry_point| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    cache: None,
                })
            });
            let (wiring, ports) = upload_layout(device, &layout);
            (bindings, advance, observe, wiring, ports)
        })?;
        Ok(Self {
            gpu,
            layout,
            bindings,
            advance,
            observe,
            wiring,
            ports,
        })
    }

    /// 実行に使用する GPU アダプターの情報を返す。
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.gpu.device.adapter_info()
    }

    /// 指定のレーンワード数と tick 数で必要なバッファーとディスパッチを検査する。
    ///
    /// 初期状態や入力を割り当てる前に呼び出せる。この検査ではバッファーを確保しない。
    ///
    /// # Errors
    ///
    /// デバイスを再利用できない場合、レーンワード数が 0 の場合、
    /// または指定した実行の寸法がデバイスの資源上限を超える場合にエラーを返す。
    pub fn check_batch(&self, lane_words: u32, ticks: u32) -> Result<(), GpuNorError> {
        self.shape(lane_words, ticks).map(|_| ())
    }

    fn shape(&self, lane_words: u32, ticks: u32) -> Result<Shape, GpuNorError> {
        self.gpu.check_available()?;
        Shape::check(&self.layout, lane_words, ticks, &self.gpu.device.limits())
            .ok_or(GpuNorError::ResourceLimit)
    }

    /// 全 tick を実行し、最終ゲート状態と各 tick の公開出力を読み戻す。
    ///
    /// 初期状態はゲート順、入力は tick、入力ポートの順に並べる。
    /// 各ゲートやポートは、レーン順に lane_words 個の 32 ビットワードで表す。
    /// tick 数が 0 の場合、空の入力を受け取り、初期状態と空の出力を返す。
    /// GPU の完了を待つため、呼び出し元のスレッドをブロックする。一度に投入した処理の
    /// 待機が 30 秒を超えた場合、このインスタンスでの実行を打ち切る。
    ///
    /// # Panics
    ///
    /// wgpu の致命的エラーを panic として処理する際、panic を中断に設定したビルドでは
    /// プロセスが終了する場合がある。
    ///
    /// # Errors
    ///
    /// レーンワード数が 0 の場合、入力や初期状態の長さが合わない場合、
    /// 資源上限を超える場合、実行や読み戻しに失敗した場合にエラーを返す。
    pub fn evaluate(
        &mut self,
        lane_words: u32,
        ticks: u32,
        initial: &[u32],
        frames: &[u32],
    ) -> Result<Trace, GpuNorError> {
        let shape = self.shape(lane_words, ticks)?;
        if initial.len() != shape.state_words as usize {
            return Err(GpuNorError::InitialStateLength);
        }
        if frames.len() != shape.input_words as usize {
            return Err(GpuNorError::InputLength);
        }
        if ticks == 0 || shape.state_words == 0 && shape.trace_words == 0 {
            return Ok(Trace {
                gates: initial.to_vec(),
                outputs: Vec::new(),
            });
        }
        let trace = scoped(&self.gpu.device, || {
            self.execute(shape, ticks, initial, frames)
        })??;
        self.gpu.check_available()?;
        Ok(trace)
    }

    fn execute(
        &self,
        shape: Shape,
        ticks: u32,
        initial: &[u32],
        frames: &[u32],
    ) -> Result<Trace, GpuNorError> {
        let device = &self.gpu.device;
        let (state_bytes, trace_bytes) =
            (byte_size(shape.state_words), byte_size(shape.trace_words));
        let states = [
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(if initial.is_empty() { &[0] } else { initial }),
                usage: STATE,
            }),
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: state_bytes.max(4),
                usage: STATE,
                mapped_at_creation: false,
            }),
        ];
        let inputs = upload(device, frames, &[0]);
        let trace = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: trace_bytes.max(4),
            usage: STATE,
            mapped_at_creation: false,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: state_bytes + trace_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // tick のパラメーターを一括転送すると、状態の向きごとにバインドグループを再利用できる。
        let mut parameters = vec![0; shape.parameter_words as usize];
        for tick in 0..ticks {
            let start = (tick * shape.parameter_stride / 4) as usize;
            parameters[start..][..PARAMETERS as usize].copy_from_slice(&[
                shape.lanes,
                shape.inputs,
                shape.gates,
                shape.ports,
                tick,
            ]);
        }
        let dimensions = upload(device, &parameters, &[0]);
        let groups = [0, 1].map(|side| {
            let mut entries: Vec<_> = [
                &states[side],
                &states[1 - side],
                &self.wiring,
                &inputs,
                &self.ports,
                &trace,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, buffer)| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &dimensions,
                    offset: 0,
                    size: wgpu::BufferSize::new(byte_size(PARAMETERS)),
                }),
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.bindings,
                entries: &entries,
            })
        });

        // コマンドを分割して、GPU に投入するコマンド数を一度に最大 512 パスに抑える。
        let mut commands =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for tick in 0..ticks {
            let group = &groups[(tick % 2) as usize];
            // 公開出力は、同じ tick のゲート更新が完了してから読む。
            for (pipeline, words) in [
                (&self.advance, shape.state_words),
                (&self.observe, shape.trace_per_tick),
            ] {
                if words != 0 {
                    let mut pass =
                        commands.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, group, &[tick * shape.parameter_stride]);
                    pass.dispatch_workgroups(words.div_ceil(WORKGROUP), 1, 1);
                }
            }
            if (tick + 1) % TICKS_PER_SUBMIT == 0 && tick + 1 != ticks {
                let submitted = self.gpu.queue.submit([commands.finish()]);
                self.gpu.wait(submitted)?;
                commands =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            }
        }
        if state_bytes != 0 {
            commands.copy_buffer_to_buffer(
                &states[(ticks % 2) as usize],
                0,
                &download,
                0,
                state_bytes,
            );
        }
        if trace_bytes != 0 {
            commands.copy_buffer_to_buffer(&trace, 0, &download, state_bytes, trace_bytes);
        }
        let submitted = self.gpu.queue.submit([commands.finish()]);
        self.gpu.readback(&download, submitted, |words| {
            let (gates, outputs) = words.split_at(shape.state_words as usize);
            Trace {
                gates: gates.to_vec(),
                outputs: outputs.to_vec(),
            }
        })
    }
}

fn upload_layout(
    device: &wgpu::Device,
    layout: &PackedNorLayout32,
) -> (wgpu::Buffer, wgpu::Buffer) {
    // Core は配線番号を source_count 未満に制限する。呼び出し側はその上限を u32 で検査済み。
    let routes: Vec<_> = layout
        .gates()
        .iter()
        .flat_map(|gate| [gate.left as u32, gate.right as u32])
        .collect();
    let ports: Vec<_> = layout.outputs().iter().map(|port| *port as u32).collect();
    (
        upload(device, &routes, &[0, 0]),
        upload(device, &ports, &[0]),
    )
}

struct GpuDevice {
    device: wgpu::Device,
    unavailable: Arc<AtomicBool>,
    queue: wgpu::Queue,
}

impl GpuDevice {
    fn new() -> Result<Self, GpuNorError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|_| GpuNorError::AdapterUnavailable)?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu
            || !adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err(GpuNorError::AdapterUnavailable);
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(GpuNorError::Device)?;
        let unavailable = Arc::new(AtomicBool::new(false));
        let device_unavailable = Arc::clone(&unavailable);
        device.set_device_lost_callback(move |_, _| {
            device_unavailable.store(true, Ordering::Release)
        });
        Ok(Self {
            device,
            unavailable,
            queue,
        })
    }

    fn check_available(&self) -> Result<(), GpuNorError> {
        if self.unavailable.load(Ordering::Acquire) {
            Err(GpuNorError::DeviceUnavailable)
        } else {
            Ok(())
        }
    }

    fn wait(&self, submission: wgpu::SubmissionIndex) -> Result<(), GpuNorError> {
        // wgpu 30 のネイティブ版は、一部のデバイス障害を PollError で返さず panic にする。
        // この呼び出しから復帰しても、そのデバイスには後続の計算を投入しない。
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(SUBMIT_TIMEOUT),
            })
        }));
        match result {
            Ok(Ok(_)) if self.unavailable.load(Ordering::Acquire) => {
                Err(GpuNorError::DeviceUnavailable)
            }
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => {
                self.unavailable.store(true, Ordering::Release);
                Err(GpuNorError::Wait(error))
            }
            Err(_) => {
                self.unavailable.store(true, Ordering::Release);
                Err(GpuNorError::DeviceUnavailable)
            }
        }
    }

    fn readback<T>(
        &self,
        download: &wgpu::Buffer,
        submitted: wgpu::SubmissionIndex,
        read: impl FnOnce(&[u32]) -> T,
    ) -> Result<T, GpuNorError> {
        let slice = download.slice(..);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.wait(submitted)?;
        receiver
            .try_recv()
            .map_err(|_| GpuNorError::Readback)?
            .map_err(|_| GpuNorError::Readback)?;
        let bytes = slice
            .get_mapped_range()
            .map_err(|_| GpuNorError::Readback)?;
        let words: &[u32] = bytemuck::try_cast_slice(&bytes).map_err(|_| GpuNorError::Readback)?;
        Ok(read(words))
    }
}

fn scoped<T>(device: &wgpu::Device, run: impl FnOnce() -> T) -> Result<T, GpuNorError> {
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = run();
    let validation = pollster::block_on(validation.pop());
    let memory = pollster::block_on(memory.pop());
    let internal = pollster::block_on(internal.pop());
    validation
        .or(memory)
        .or(internal)
        .map_or(Ok(result), |error| Err(GpuNorError::Gpu(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use circuitgame2_core::netlist::{ClockedNor, NorGate, NorNetlist, Wire};

    #[test]
    fn shape_checks_dispatch_and_dynamic_offsets_before_allocation() {
        let layout = ClockedNor::check(NorNetlist {
            input_count: 0,
            gates: vec![NorGate {
                left: Wire::Gate(0),
                right: Wire::Gate(0),
            }],
            outputs: Vec::new(),
        })
        .unwrap()
        .packed_layout32()
        .unwrap();
        let mut limits = wgpu::Limits::defaults();
        limits.max_compute_workgroups_per_dimension = 1;
        assert!(Shape::check(&layout, WORKGROUP + 1, 1, &limits).is_none());
        assert!(Shape::check(&layout, 0, 1, &limits).is_none());
        assert!(Shape::check(&layout, 1, u32::MAX, &limits).is_none());

        let shape = Shape::check(&layout, 1, 2, &limits).unwrap();
        assert_eq!(
            shape.parameter_stride % limits.min_storage_buffer_offset_alignment,
            0
        );
        assert_eq!(
            byte_size(shape.parameter_words),
            u64::from(shape.parameter_stride) + byte_size(PARAMETERS)
        );
    }
}
