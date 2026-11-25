//! 検査済みの組み合わせ回路をワード単位で並列に評価する。

use circuitgame2_core::netlist::PackedCombinationalLayout32;

use super::{
    GpuDevice, GpuNorError, STATE, STORAGE, WORKGROUP, byte_size, scoped, upload, upload_layout,
};

const BINDINGS: u32 = 5;
const PARAMETERS: u32 = 4;

struct Shape {
    inputs: u32,
    gates: u32,
    ports: u32,
    input_words: u32,
    wire_words: u32,
    output_words: u32,
    groups: u32,
}

impl Shape {
    fn check(
        layout: &PackedCombinationalLayout32,
        lanes: u32,
        limits: &wgpu::Limits,
    ) -> Option<Self> {
        let layout = layout.layout();
        let inputs = u32::try_from(layout.input_count()).ok()?;
        let gates = u32::try_from(layout.gates().len()).ok()?;
        let ports = u32::try_from(layout.outputs().len()).ok()?;
        let sources = u32::try_from(layout.source_count()).ok()?;
        let input_words = inputs.checked_mul(lanes)?;
        let wire_words = sources.checked_mul(lanes)?;
        let output_words = ports.checked_mul(lanes)?;
        let route_words = gates.checked_mul(2)?;
        let groups = lanes.div_ceil(WORKGROUP);
        let storage_limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let host_limit = isize::MAX as u64;
        if lanes == 0
            || limits.max_bind_groups == 0
            || limits.max_bindings_per_bind_group < BINDINGS
            || limits.max_storage_buffers_per_shader_stage < BINDINGS
            || limits.max_buffers_and_acceleration_structures_per_shader_stage < BINDINGS
            || limits.max_non_sampler_bindings < BINDINGS
            || limits.max_compute_invocations_per_workgroup < WORKGROUP
            || limits.max_compute_workgroup_size_x < WORKGROUP
            || groups > limits.max_compute_workgroups_per_dimension
            || u64::from(groups) * u64::from(WORKGROUP) > u64::from(u32::MAX) + 1
            || [
                wire_words,
                output_words,
                route_words.max(2),
                ports.max(1),
                PARAMETERS,
            ]
            .into_iter()
            .any(|words| byte_size(words) > storage_limit.min(host_limit))
        {
            return None;
        }
        Some(Self {
            inputs,
            gates,
            ports,
            input_words,
            wire_words,
            output_words,
            groups,
        })
    }
}

/// Core が依存順を検査した組み合わせ NOR 回路をデスクトップ GPU で評価する。
///
/// 1個の32ビットワードに独立したケースを32件詰める。最下位ビットがレーン0に当たる。
/// 配線と計算パイプラインは GPU に保持し、入力を変えて繰り返し評価できる。
pub struct GpuCombinationalNor32 {
    gpu: GpuDevice,
    layout: PackedCombinationalLayout32,
    pipeline: wgpu::ComputePipeline,
    bindings: wgpu::BindGroupLayout,
    wiring: wgpu::Buffer,
    ports: wgpu::Buffer,
}

impl GpuCombinationalNor32 {
    /// Vulkan GPU を選び、検査済みの配線を転送する。
    ///
    /// # Errors
    ///
    /// GPU を使えない場合、配置が資源上限を超える場合、転送やパイプライン作成に失敗した場合にエラーを返す。
    pub fn new(layout: PackedCombinationalLayout32) -> Result<Self, GpuNorError> {
        let gpu = GpuDevice::new()?;
        let device = &gpu.device;
        Shape::check(&layout, 1, &device.limits()).ok_or(GpuNorError::ResourceLimit)?;
        let (pipeline, bindings, wiring, ports) = scoped(device, || {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("NCG2 combinational NOR"),
                source: wgpu::ShaderSource::Wgsl(include_str!("combinational_nor.wgsl").into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &shader,
                entry_point: Some("evaluate"),
                compilation_options: Default::default(),
                cache: None,
            });
            let bindings = pipeline.get_bind_group_layout(0);
            let (wiring, ports) = upload_layout(device, layout.layout());
            (pipeline, bindings, wiring, ports)
        })?;
        gpu.check_available()?;
        Ok(Self {
            gpu,
            layout,
            pipeline,
            bindings,
            wiring,
            ports,
        })
    }

    /// 評価に使用する GPU アダプターの情報を返す。
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.gpu.device.adapter_info()
    }

    /// 指定したレーンワード数で、GPU のバッファーとディスパッチの上限を検査する。
    ///
    /// # Errors
    ///
    /// デバイスを再利用できない場合、レーンワード数が0の場合、または資源上限を超える場合にエラーを返す。
    pub fn check_batch(&self, lane_words: u32) -> Result<(), GpuNorError> {
        self.shape(lane_words).map(|_| ())
    }

    fn shape(&self, lane_words: u32) -> Result<Shape, GpuNorError> {
        self.gpu.check_available()?;
        Shape::check(&self.layout, lane_words, &self.gpu.device.limits())
            .ok_or(GpuNorError::ResourceLimit)
    }

    /// 入力ポート、レーンワードの順に受け取り、公開出力ポート、レーンワードの順に返す。
    ///
    /// 1回のディスパッチで全ゲートを評価する。公開出力がない場合は空の列を返す。
    /// シェーダーの完了を待つため、呼び出したスレッドをブロックする。
    ///
    /// # Errors
    ///
    /// 入力の長さが配置と合わない場合、実行規模が上限を超える場合、または GPU の実行や読み戻しに失敗した場合にエラーを返す。
    ///
    /// # Panics
    ///
    /// wgpu の致命的エラーを panic として処理する際、panic を中断に設定したビルドでは
    /// プロセスが終了する場合がある。
    pub fn evaluate(&mut self, lane_words: u32, inputs: &[u32]) -> Result<Vec<u32>, GpuNorError> {
        let shape = self.shape(lane_words)?;
        if inputs.len() != shape.input_words as usize {
            return Err(GpuNorError::InputLength);
        }
        if shape.output_words == 0 {
            return Ok(Vec::new());
        }
        let result = scoped(&self.gpu.device, || self.execute(lane_words, shape, inputs))??;
        self.gpu.check_available()?;
        Ok(result)
    }

    fn execute(
        &self,
        lane_words: u32,
        shape: Shape,
        inputs: &[u32],
    ) -> Result<Vec<u32>, GpuNorError> {
        let device = &self.gpu.device;
        let wires = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: byte_size(shape.wire_words),
            usage: STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output_bytes = byte_size(shape.output_words);
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: output_bytes,
            usage: STATE,
            mapped_at_creation: false,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: output_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let dimensions = upload(
            device,
            &[lane_words, shape.inputs, shape.gates, shape.ports],
            &[0],
        );
        let entries = [&wires, &self.wiring, &self.ports, &output, &dimensions]
            .into_iter()
            .enumerate()
            .map(|(index, buffer)| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bindings,
            entries: &entries,
        });

        let mut commands =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = commands.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(shape.groups, 1, 1);
        }
        commands.copy_buffer_to_buffer(&output, 0, &download, 0, output_bytes);
        if !inputs.is_empty() {
            self.gpu
                .queue
                .write_buffer(&wires, 0, bytemuck::cast_slice(inputs));
        }
        let submitted = self.gpu.queue.submit([commands.finish()]);
        self.gpu.readback(&download, submitted, <[u32]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use circuitgame2_core::netlist::{NorNetlist, Wire};

    #[test]
    fn shape_checks_dispatch_and_flat_wire_extent() {
        let layout = NorNetlist {
            input_count: 1,
            gates: Vec::new(),
            outputs: vec![Wire::Input(0)],
        }
        .packed_combinational_layout32()
        .unwrap();
        let mut limits = wgpu::Limits::defaults();
        limits.max_compute_workgroups_per_dimension = 1;
        assert!(Shape::check(&layout, 0, &limits).is_none());
        assert!(Shape::check(&layout, WORKGROUP + 1, &limits).is_none());
        assert!(Shape::check(&layout, u32::MAX / 3 + 1, &limits).is_none());
        assert!(Shape::check(&layout, WORKGROUP, &limits).is_some());
        limits.max_storage_buffer_binding_size = byte_size(WORKGROUP * 3) - 1;
        assert!(Shape::check(&layout, WORKGROUP, &limits).is_none());
    }
}
