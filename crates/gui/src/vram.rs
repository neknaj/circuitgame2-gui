//! 公開 VRAM の確定済み内容を、現在の表示範囲だけ読み取る。

use circuitgame2_web::Simulation;
use eframe::egui;

const PAGE_BITS: usize = 128;
const ROW_BITS: usize = 32;

pub(crate) struct VramPanel {
    names: Vec<String>,
    selected: usize,
    offset: usize,
}

impl VramPanel {
    pub(crate) fn new(vm: &Simulation) -> Self {
        Self {
            names: vm.vram_names(),
            selected: 0,
            offset: 0,
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, vm: &Simulation) {
        let Some(name) = self.names.get(self.selected) else {
            return;
        };
        ui.separator();
        ui.heading("VRAM");
        let previous = self.selected;
        let label = ui.label("Memory");
        egui::ComboBox::from_id_salt("vram-name")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (index, name) in self.names.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, index, name);
                }
            })
            .response
            .labelled_by(label.id);
        if self.selected != previous {
            self.offset = 0;
        }

        let name = &self.names[self.selected];
        let Some(capacity) = vm.vram_bit_count(name) else {
            return;
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.offset != 0, egui::Button::new("Previous"))
                .clicked()
            {
                self.offset -= PAGE_BITS;
            }
            if ui
                .add_enabled(
                    capacity.saturating_sub(self.offset) > PAGE_BITS,
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                self.offset += PAGE_BITS;
            }
        });
        if let Some(bits) = vm.vram_page(name, self.offset, PAGE_BITS) {
            let end = self.offset + bits.len();
            if end == self.offset {
                ui.label("0 bits");
                return;
            }
            ui.label(format!("Bits {}–{end} of {capacity}", self.offset + 1));
            // アドレス順のビット列を画面幅に収まる長さで折り返す。
            for start in (0..bits.len()).step_by(ROW_BITS) {
                ui.monospace(&bits[start..(start + ROW_BITS).min(bits.len())]);
            }
        }
    }
}
