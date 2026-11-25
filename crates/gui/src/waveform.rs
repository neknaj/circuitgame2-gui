//! 入力と出力のビットを、確定した tick 順に表示する。

use std::collections::VecDeque;

use circuitgame2_web::Simulation;
use eframe::egui::{self, Align2, Color32, DragValue, FontId, Sense, Stroke, pos2, vec2};

const MAX_SAMPLES: usize = 128;
const SAMPLE_BYTES: usize = 1024 * 1024;
const ROW_HEIGHT: f32 = 24.0;
const MAX_PANEL_HEIGHT: f32 = 320.0;

pub(crate) struct Waveform {
    samples: VecDeque<Box<[u8]>>,
    capacity: usize,
    input_width: usize,
    output_width: usize,
    window: usize,
}

impl Waveform {
    pub(crate) fn new(input_width: usize, output_width: usize) -> Self {
        let bytes_per_tick = (input_width + output_width).div_ceil(8).max(1);
        let capacity = (SAMPLE_BYTES / bytes_per_tick).clamp(1, MAX_SAMPLES);
        Self {
            samples: VecDeque::new(),
            capacity,
            input_width,
            output_width,
            window: capacity.min(32),
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

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, tick: u64, vm: &Simulation) {
        let channels = self.input_width + self.output_width;
        if channels == 0 || self.samples.is_empty() {
            return;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Waveform");
            let label = ui.label("Ticks shown");
            ui.add(DragValue::new(&mut self.window).range(1..=self.capacity))
                .labelled_by(label.id);
        });
        let count = self.samples.len().min(self.window);
        let start = self.samples.len() - count;
        let first = tick - (count - 1) as u64;
        let width = (ui.available_width() - ui.spacing().scroll.allocated_width()).max(1.0);
        let label_width = (width * 0.28).min(168.0);
        let (header, painter) = ui.allocate_painter(vec2(width, 24.0), Sense::hover());
        let plot = header
            .rect
            .with_min_x(header.rect.left() + label_width + 6.0);
        if plot.width() < 12.0 {
            return;
        }
        let dx = plot.width() / count as f32;
        let font = FontId::monospace(11.0);
        let tick_label_width = painter
            .layout_no_wrap(tick.to_string(), font.clone(), ui.visuals().text_color())
            .size()
            .x
            + 12.0;
        let stride = (tick_label_width / dx).ceil().max(1.0) as usize;
        let row_spacing = ui.spacing().item_spacing.y;
        let panel_height =
            ((ROW_HEIGHT + row_spacing) * channels as f32 - row_spacing).min(MAX_PANEL_HEIGHT);
        let hit_rect = egui::Rect::from_min_max(
            plot.left_top(),
            pos2(
                plot.right(),
                header.rect.bottom() + row_spacing + panel_height,
            ),
        )
        .intersect(ui.clip_rect());
        // カーソルが指すサンプルを全行で共有する。
        let cursor = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|&pointer| hit_rect.contains(pointer))
            .map(|pointer| (((pointer.x - plot.left()) / dx) as usize).min(count - 1));
        let cursor_x = cursor.map(|index| plot.left() + (index as f32 + 0.5) * dx);
        let tick_x = |index| plot.left() + index as f32 * dx;

        painter.text(
            header.rect.left_center(),
            Align2::LEFT_CENTER,
            "Signal",
            font.clone(),
            ui.visuals().text_color(),
        );
        for index in (0..count).step_by(stride) {
            let x = tick_x(index);
            if index != 0 && x + tick_label_width > plot.right() {
                break;
            }
            painter.text(
                pos2(x, header.rect.center().y),
                Align2::LEFT_CENTER,
                first + index as u64,
                font.clone(),
                ui.visuals().text_color(),
            );
        }

        egui::ScrollArea::vertical()
            .id_salt("waveform-signals")
            .max_height(MAX_PANEL_HEIGHT)
            .show_rows(ui, ROW_HEIGHT, channels, |ui, visible| {
                for bit in visible {
                    let input = bit < self.input_width;
                    let (name, direction) = if input {
                        (vm.input_signal_name(bit), "In")
                    } else {
                        (vm.output_signal_name(bit - self.input_width), "Out")
                    };
                    let value = level(&self.samples[start + cursor.unwrap_or(count - 1)], bit);
                    let color = signal_color(ui, input);
                    let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::hover());
                    let label = format!(
                        "{direction}  {}  {}",
                        u8::from(value),
                        name.unwrap_or_default()
                    );
                    ui.place(
                        row.with_max_x(row.left() + label_width),
                        egui::Label::new(egui::RichText::new(&label).monospace().color(color))
                            .truncate(),
                    )
                    .on_hover_text(label);
                    let painter = ui.painter().with_clip_rect(row.with_min_x(plot.left()));
                    if !input && bit == self.input_width {
                        painter.hline(
                            plot.x_range(),
                            row.top(),
                            ui.visuals().widgets.noninteractive.bg_stroke,
                        );
                    }
                    let grid =
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
                    for index in (0..count).step_by(stride) {
                        painter.vline(tick_x(index), row.y_range(), grid);
                    }
                    let stroke = Stroke::new(1.7, color);
                    let y = |high| {
                        if high {
                            row.top() + 6.0
                        } else {
                            row.bottom() - 6.0
                        }
                    };
                    let mut previous: Option<f32> = None;
                    for (index, sample) in self.samples.iter().skip(start).enumerate() {
                        let current = y(level(sample, bit));
                        let x = tick_x(index);
                        if let Some(old) = previous.filter(|&old| old != current) {
                            painter.vline(x, old.min(current)..=old.max(current), stroke);
                        }
                        painter.hline(x..=tick_x(index + 1), current, stroke);
                        previous = Some(current);
                    }
                    if let Some(x) = cursor_x {
                        painter.vline(
                            x,
                            row.y_range(),
                            Stroke::new(1.0, ui.visuals().text_color()),
                        );
                    }
                }
            });
        ui.label(if let Some(index) = cursor {
            format!("Tick {}", first + index as u64)
        } else {
            format!("Ticks {first}–{tick}")
        });
    }
}

fn signal_color(ui: &egui::Ui, input: bool) -> Color32 {
    match (ui.visuals().dark_mode, input) {
        (true, true) => Color32::from_rgb(96, 192, 248),
        (true, false) => Color32::from_rgb(255, 177, 97),
        (false, true) => Color32::from_rgb(0, 92, 154),
        (false, false) => Color32::from_rgb(157, 70, 0),
    }
}

fn level(sample: &[u8], bit: usize) -> bool {
    sample[bit / 8] & (1 << (bit % 8)) != 0
}

#[cfg(test)]
mod tests {
    use super::{MAX_SAMPLES, Waveform, level};

    #[test]
    fn names_follow_the_declaration_and_the_lsb_first_layout() {
        let vm = circuitgame2_web::Simulation::compile(
            "(using nor) (module main (in enabled (data (bits 2))) (out enabled (data (bits 2))))",
        )
        .unwrap();

        for bit in 0..3 {
            let expected = ["enabled", "data[0]", "data[1]"][bit];
            assert_eq!(vm.input_signal_name(bit).as_deref(), Some(expected));
            assert_eq!(vm.output_signal_name(bit).as_deref(), Some(expected));
        }
        assert_eq!(vm.input_signal_name(3), None);
        assert_eq!(vm.output_signal_name(3), None);
    }

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
