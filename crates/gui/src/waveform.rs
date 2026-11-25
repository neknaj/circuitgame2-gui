//! 入力と出力のビットを、確定した tick 順に表示する。

use std::collections::VecDeque;

use eframe::egui::{self, Align2, DragValue, FontId, Sense, Stroke, pos2, vec2};

const MAX_SAMPLES: usize = 128;
const SAMPLE_BYTES: usize = 1024 * 1024;

pub(crate) struct Waveform {
    samples: VecDeque<Box<[u8]>>,
    capacity: usize,
    input_width: usize,
    output_width: usize,
    input_bit: usize,
    output_bit: usize,
}

impl Waveform {
    pub(crate) fn new(input_width: usize, output_width: usize) -> Self {
        let bytes_per_tick = (input_width + output_width).div_ceil(8).max(1);
        Self {
            samples: VecDeque::new(),
            capacity: (SAMPLE_BYTES / bytes_per_tick).clamp(1, MAX_SAMPLES),
            input_width,
            output_width,
            input_bit: 1,
            output_bit: 1,
        }
    }

    pub(crate) fn reset(&mut self, inputs: &[bool], outputs: &str) {
        self.samples.clear();
        self.record(inputs, outputs);
    }

    pub(crate) fn record(&mut self, inputs: &[bool], outputs: &str) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        // サンプルにはポート順の入出力を詰め、回路幅が大きい場合も履歴の容量を抑える。
        let mut sample = vec![0; (self.input_width + self.output_width).div_ceil(8)];
        for (index, high) in inputs
            .iter()
            .copied()
            .chain(outputs.bytes().map(|bit| bit == b'1'))
            .enumerate()
        {
            if high {
                sample[index / 8] |= 1 << (index % 8);
            }
        }
        self.samples.push_back(sample.into_boxed_slice());
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, tick: u64) {
        if self.input_width == 0 && self.output_width == 0 {
            return;
        }
        let first = tick - (self.samples.len() - 1) as u64;
        ui.separator();
        ui.heading("Waveform");
        ui.label(format!("Ticks {first}–{tick}"));
        if self.input_width != 0 {
            pick_bit(ui, "Input bit", &mut self.input_bit, self.input_width);
            self.show_bit(ui, first, self.input_bit - 1);
        }
        if self.output_width != 0 {
            pick_bit(ui, "Output bit", &mut self.output_bit, self.output_width);
            self.show_bit(ui, first, self.input_width + self.output_bit - 1);
        }
    }

    fn show_bit(&self, ui: &mut egui::Ui, first_tick: u64, bit: usize) {
        let count = self.samples.len();
        let (response, painter) =
            ui.allocate_painter(vec2(ui.available_width(), 38.0), Sense::hover());
        let graph = response.rect.shrink2(vec2(2.0, 5.0));
        let dx = graph.width() / count as f32;
        let font = FontId::monospace(11.0);
        let color = ui.visuals().text_color();
        let signal = Stroke::new(2.0, color);
        let y = |high| if high { graph.top() } else { graph.bottom() };
        let mut last_y = None;
        for (index, sample) in self.samples.iter().enumerate() {
            let x = graph.left() + index as f32 * dx;
            let current_y = y(level(sample, bit));
            if let Some(previous_y) = last_y {
                painter.line_segment([pos2(x, previous_y), pos2(x, current_y)], signal);
            }
            painter.line_segment([pos2(x, current_y), pos2(x + dx, current_y)], signal);
            last_y = Some(current_y);
        }
        if let Some(pointer) = response.hover_pos() {
            let index = (((pointer.x - graph.left()) / dx) as usize).min(count - 1);
            let x = graph.left() + (index as f32 + 0.5) * dx;
            let high = level(&self.samples[index], bit);
            painter.text(
                pos2(x, graph.center().y),
                Align2::CENTER_CENTER,
                u8::from(high),
                font,
                color,
            );
            response.on_hover_text(format!(
                "Tick {}: {}",
                first_tick + index as u64,
                u8::from(high)
            ));
        }
        if let Some(last) = self.samples.back() {
            ui.label(format!("Latest value: {}", u8::from(level(last, bit))));
        }
    }
}

fn pick_bit(ui: &mut egui::Ui, name: &str, bit: &mut usize, width: usize) {
    ui.horizontal(|ui| {
        let label = ui.label(name);
        ui.add(DragValue::new(bit).range(1..=width))
            .labelled_by(label.id);
    });
}

fn level(sample: &[u8], bit: usize) -> bool {
    sample[bit / 8] & (1 << (bit % 8)) != 0
}

#[cfg(test)]
mod tests {
    use super::{MAX_SAMPLES, Waveform, level};

    #[test]
    fn history_preserves_port_order_and_replaces_old_samples() {
        let mut history = Waveform::new(9, 2);
        let mut inputs = [false; 9];
        inputs[8] = true;
        history.reset(&inputs, "01");
        let first = &history.samples[0];
        assert!(level(first, 8));
        assert!(!level(first, 9));
        assert!(level(first, 10));

        for _ in 0..MAX_SAMPLES {
            history.record(&[false; 9], "10");
        }
        assert_eq!(history.samples.len(), MAX_SAMPLES);
        assert!(!level(&history.samples[0], 8));
        assert!(level(&history.samples[0], 9));
        assert!(!level(&history.samples[0], 10));

        history.reset(&inputs, "01");
        assert_eq!(history.samples.len(), 1);
        assert!(level(&history.samples[0], 10));
    }

    #[test]
    fn wide_circuits_keep_a_bounded_history() {
        let history = Waveform::new(1 << 20, 1 << 20);
        assert_eq!(history.capacity, 4);
    }
}
