//! デスクトップで複数の入力列を GPU に送り、ケースごとの公開出力を表示する。

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

use circuitgame2_core::netlist::ClockedNor;
use circuitgame2_gpu::GpuNor32;
use circuitgame2_web::CompileError;
use eframe::egui;

pub(crate) struct GpuBatch {
    input: String,
    job: Option<Receiver<Result<BatchOutput, CompileError>>>,
    discard_job: bool,
    output: Option<BatchOutput>,
    error: Option<CompileError>,
    case: usize,
    tick: usize,
}

impl Default for GpuBatch {
    fn default() -> Self {
        Self {
            input: "0 1 0\n1 0 1".into(),
            job: None,
            discard_job: false,
            output: None,
            error: None,
            case: 0,
            tick: 0,
        }
    }
}

impl GpuBatch {
    pub(crate) fn invalidate(&mut self) {
        self.output = None;
        self.error = None;
        // 処理中のジョブは完了まで保持し、同時に複数の GPU 実行を開始しない。
        self.discard_job = self.job.is_some();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) -> bool {
        ui.collapsing("GPU batch", |ui| {
            ui.label("One case per line. Separate tick inputs with spaces. Use - for a circuit with no inputs.");
            ui.label("Requires Vulkan and one-bit ports. Built-in latches, RAM, and external devices are unavailable.");
            if ui.add(egui::TextEdit::multiline(&mut self.input).desired_rows(3)
                    .code_editor().desired_width(f32::INFINITY)).changed() {
                self.invalidate();
            }
            let run = ui.add_enabled(self.job.is_none(), egui::Button::new("Run on GPU")).clicked();
            if self.job.is_some() {
                ui.label("Running on GPU…");
            }
            if let Some(output) = &self.output {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.case).range(1..=output.cases).prefix("Case "));
                    ui.add(egui::DragValue::new(&mut self.tick).range(0..=output.ticks).prefix("Tick "));
                });
                let bits = output.bits(self.case - 1, self.tick);
                if bits.is_empty() {
                    ui.label("No output ports");
                } else {
                    egui::ScrollArea::horizontal().id_salt("gpu-output").show(ui, |ui| {
                        ui.monospace(bits);
                    });
                }
            }
            run
        }).body_returned.unwrap_or(false)
    }

    pub(crate) fn start(
        &mut self,
        ctx: egui::Context,
        compile: impl FnOnce() -> Result<ClockedNor, CompileError> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        self.invalidate();
        let input = self.input.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let repaint = ctx.clone();
        let job = thread::Builder::new()
            .name("ncg2-gpu-batch".into())
            .spawn(move || {
                let result = compile().and_then(|circuit| evaluate(&circuit, &input));
                let _ = sender.send(result);
                repaint.request_repaint();
            });
        match job {
            Ok(_) => {
                self.job = Some(receiver);
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(error) => {
                self.error = Some(format!("GPU の作業スレッドを起動できない。{error}").into())
            }
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                // 作業スレッドが通知を送らずに終了した場合も、切断を検出する。
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => {
                Err("GPU の作業スレッドから結果を受け取れなかった。"
                    .to_owned()
                    .into())
            }
        };
        self.job = None;
        if std::mem::take(&mut self.discard_job) {
            return;
        }
        match result {
            Ok(output) => {
                self.case = 1;
                self.tick = 0;
                self.output = Some(output);
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(crate) fn error(&self) -> Option<&CompileError> {
        self.error.as_ref()
    }
}

struct BatchOutput {
    cases: usize,
    ticks: usize,
    words: usize,
    reset: Vec<bool>,
    trace: Vec<u32>,
}

impl BatchOutput {
    fn bits(&self, case: usize, tick: usize) -> String {
        if tick == 0 {
            return self.reset.iter().copied().map(bit_char).collect();
        }
        let width = self.reset.len();
        let word = case / 32;
        let mask = 1u32 << (case % 32);
        (0..width)
            .map(|port| {
                let offset = ((tick - 1) * width + port) * self.words + word;
                bit_char(self.trace[offset] & mask != 0)
            })
            .collect()
    }
}

fn bit_char(bit: bool) -> char {
    if bit { '1' } else { '0' }
}

struct BatchInput<'a> {
    text: &'a str,
    width: usize,
    cases: usize,
    ticks: usize,
    words: usize,
}

impl<'a> BatchInput<'a> {
    fn parse(text: &'a str, width: usize) -> Result<Self, String> {
        let lines = || text.lines().filter(|line| !line.trim().is_empty());
        let cases = lines().count();
        let ticks = lines()
            .next()
            .map_or(0, |line| line.split_whitespace().count());
        if cases == 0 {
            return Err("入力列を1ケース以上指定する。".into());
        }
        for (case, line) in lines().enumerate() {
            if line.split_whitespace().count() != ticks {
                return Err(format!(
                    "ケース {} の tick 数を先頭のケースと揃える。",
                    case + 1
                ));
            }
            for (tick, frame) in line.split_whitespace().enumerate() {
                let valid = if width == 0 {
                    frame == "-"
                } else {
                    frame.len() == width && frame.bytes().all(|bit| matches!(bit, b'0' | b'1'))
                };
                if !valid {
                    return Err(format!(
                        "ケース {} の tick {} の入力が回路と合わない。",
                        case + 1,
                        tick + 1
                    ));
                }
            }
        }
        Ok(Self {
            text,
            width,
            cases,
            ticks,
            words: cases.div_ceil(32),
        })
    }

    fn pack(&self) -> Result<Vec<u32>, String> {
        let count = self
            .ticks
            .checked_mul(self.width)
            .and_then(|count| count.checked_mul(self.words))
            .ok_or("入力を格納できる範囲を超えた。")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| "入力の領域を確保できない。")?;
        values.resize(count, 0u32);
        if self.width != 0 {
            for (case, line) in self
                .text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .enumerate()
            {
                for (tick, frame) in line.split_whitespace().enumerate() {
                    for (port, bit) in frame.bytes().enumerate() {
                        let offset = (tick * self.width + port) * self.words + case / 32;
                        values[offset] |= u32::from(bit == b'1') << (case % 32);
                    }
                }
            }
        }
        Ok(values)
    }
}

fn evaluate(circuit: &ClockedNor, text: &str) -> Result<BatchOutput, CompileError> {
    let layout = circuit
        .packed_layout32()
        .ok_or("配線番号を GPU 向けに割り当てられない。".to_owned())?;
    let inputs = BatchInput::parse(text, layout.input_count())?;
    let tick_count =
        u32::try_from(inputs.ticks).map_err(|_| "tick 数が GPU の上限を超えた。".to_owned())?;
    let lane_words =
        u32::try_from(inputs.words).map_err(|_| "ケース数が GPU の上限を超えた。".to_owned())?;
    let mut gpu =
        GpuNor32::new(layout).map_err(|error| format!("GPU を初期化できない。{error:?}"))?;
    gpu.check_batch(lane_words, tick_count)
        .map_err(|error| format!("GPU で実行するバッチの寸法を検査できない。{error:?}"))?;
    let frames = inputs.pack()?;
    let reset = circuit.reset();
    let count = reset
        .gates
        .len()
        .checked_mul(inputs.words)
        .ok_or("初期状態を格納できる範囲を超えた。".to_owned())?;
    let mut initial = Vec::new();
    initial
        .try_reserve_exact(count)
        .map_err(|_| "初期状態の領域を確保できない。".to_owned())?;
    for bit in reset.gates {
        let word = if bit { u32::MAX } else { 0 };
        initial.resize(initial.len() + inputs.words, word);
    }
    let trace = gpu
        .evaluate(lane_words, tick_count, &initial, &frames)
        .map_err(|error| format!("GPU で実行できない。{error:?}"))?;
    Ok(BatchOutput {
        cases: inputs.cases,
        ticks: inputs.ticks,
        words: inputs.words,
        reset: reset.outputs,
        trace: trace.outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_across_two_words_keep_their_own_time_series() {
        let text: String = (0..33)
            .map(|case| match case {
                0 => "10 00\n",
                31 => "00 10\n",
                32 => "01 11\n",
                _ => "00 00\n",
            })
            .collect();
        let packed = BatchInput::parse(&text, 2).unwrap();
        let values = packed.pack().unwrap();
        assert_eq!(values, [1, 0, 0, 1, 1 << 31, 1, 0, 1]);

        let result = BatchOutput {
            cases: packed.cases,
            ticks: packed.ticks,
            words: packed.words,
            reset: vec![true, false],
            trace: values,
        };
        assert_eq!(result.bits(0, 0), "10");
        assert_eq!(result.bits(0, 1), "10");
        assert_eq!(result.bits(31, 2), "10");
        assert_eq!(result.bits(32, 1), "01");
        assert_eq!(result.bits(32, 2), "11");
        assert!(BatchInput::parse("0 1\n1", 1).is_err());
        assert!(BatchInput::parse("1", 0).is_err());
        assert!(
            BatchInput::parse("- -", 0)
                .unwrap()
                .pack()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    #[ignore = "Vulkan GPU が必要"]
    fn edited_source_runs_as_independent_cases() {
        let circuit = circuitgame2_web::compile_gpu_source(
            "(using nor) (module main (in (a (bits 1))) (out (q (bits 1))) (def q (nor a a)))",
        )
        .unwrap();
        let output = evaluate(&circuit, "0 1 0\n1 0 1").unwrap();
        for (case, expected) in [["0", "1", "0", "1"], ["0", "0", "1", "0"]]
            .into_iter()
            .enumerate()
        {
            for (tick, bits) in expected.into_iter().enumerate() {
                assert_eq!(output.bits(case, tick), bits);
            }
        }
    }

    #[test]
    fn changing_the_source_discards_an_in_flight_diagnostic() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut batch = GpuBatch {
            job: Some(receiver),
            ..GpuBatch::default()
        };
        batch.invalidate();
        sender.send(Err("古いエラー".to_owned().into())).unwrap();
        batch.poll(&egui::Context::default());
        assert!(batch.error().is_none());
        assert!(batch.job.is_none());
    }
}
