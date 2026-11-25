//! 回路本文と、その本文から作ったシミュレーターを保持する。

use std::ops::Range;

use circuitgame2_web::{CompileError, Simulation};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, RichText, TextEdit};

use crate::project::Project;

const EXAMPLE: &str = include_str!("../../../site/examples/main.ncg2");

/// 編集した NCG2 ソースを読み込み、入力と経過 tick を操作する画面。
pub struct Workbench {
    sources: Sources,
    selected: usize,
    project_generation: u64,
    simulation: Option<LoadedCircuit>,
    error: Option<CompileError>,
    pending_jump: Option<(usize, Range<usize>)>,
}

enum Sources {
    Single(String),
    Project(Project),
}

struct LoadedCircuit {
    vm: Simulation,
    inputs: Vec<bool>,
    outputs: String,
    tick: u64,
}

impl Default for Workbench {
    fn default() -> Self {
        let mut app = Self {
            sources: Sources::Single(EXAMPLE.to_owned()),
            selected: 0,
            project_generation: 0,
            simulation: None,
            error: None,
            pending_jump: None,
        };
        app.load_source();
        app
    }
}

impl Workbench {
    pub(crate) fn open_project(&mut self, project: Project) {
        self.sources = Sources::Project(project);
        self.selected = 0;
        self.project_generation = self.project_generation.wrapping_add(1);
        self.load_source();
    }

    fn show_sources(&mut self, ui: &mut egui::Ui) {
        let source = match &mut self.sources {
            Sources::Single(source) => Some(source),
            Sources::Project(project) => {
                let label = ui.label("Project source");
                let selected = project.paths.get(self.selected).map_or("—", String::as_str);
                egui::ComboBox::from_id_salt("project-source")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (index, path) in project.paths.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, index, path);
                        }
                    })
                    .response
                    .labelled_by(label.id);
                project.texts.get_mut(self.selected)
            }
        };

        if let Some(source) = source {
            let label = ui.label("Source");
            egui::ScrollArea::vertical()
                .id_salt(("source-scroll", self.project_generation, self.selected))
                .max_height(320.0)
                .show(ui, |ui| {
                    let mut output = TextEdit::multiline(source)
                        // 開き直した別のプロジェクトに、元の編集履歴を引き継がない。
                        .id_salt(("source-editor", self.project_generation, self.selected))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(12)
                        .show(ui);
                    output.response.response = output.response.response.labelled_by(label.id);
                    if output.response.changed() {
                        self.simulation = None;
                        self.error = None;
                        self.pending_jump = None;
                    }
                    if let Some((file, span)) = self.pending_jump.take()
                        && file == self.selected
                        && let Some(range) = selection_for_bytes(source, span)
                    {
                        output.state.cursor.set_char_range(Some(range));
                        output.state.store(ui.ctx(), output.response.id);
                        output.response.request_focus();
                        let cursor = output.galley.pos_from_cursor(range.secondary);
                        ui.scroll_to_rect(cursor.translate(output.galley_pos.to_vec2()), None);
                        ui.ctx().request_repaint();
                    }
                });
        }
    }

    fn show_error(&mut self, ui: &mut egui::Ui) {
        let Some(error) = &self.error else {
            return;
        };
        ui.label(RichText::new(error.to_string()).color(ui.visuals().error_fg_color));
        let Some((path, span)) = error.source_location() else {
            return;
        };
        let file = match &self.sources {
            Sources::Single(_) => Some(0),
            Sources::Project(project) => {
                project.paths.iter().position(|candidate| candidate == path)
            }
        };
        if let Some(file) = file
            && ui.button("Go to error").clicked()
        {
            self.selected = file;
            self.pending_jump = Some((file, span.clone()));
            ui.ctx().request_repaint();
        }
    }

    fn load_source(&mut self) {
        self.simulation = None;
        self.error = None;
        self.pending_jump = None;
        let result = match &self.sources {
            Sources::Single(source) => Simulation::compile(source),
            Sources::Project(project) => project.compile(),
        };
        match result {
            Ok(mut vm) => {
                let inputs = vec![false; vm.input_width()];
                let outputs = vm.reset();
                self.simulation = Some(LoadedCircuit {
                    vm,
                    inputs,
                    outputs,
                    tick: 0,
                });
            }
            Err(error) => self.error = Some(error),
        }
    }
}

impl LoadedCircuit {
    fn reset(&mut self) {
        self.outputs = self.vm.reset();
        self.tick = 0;
    }

    fn show_inputs(&mut self, ui: &mut egui::Ui) {
        let per_row = (ui.available_width() / 88.0).clamp(1.0, 16.0) as usize;
        let rows = self.inputs.len().div_ceil(per_row);
        let height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt("input-switches")
            .max_height(144.0)
            .show_rows(ui, height, rows, |ui, visible| {
                for row in visible {
                    let start = row * per_row;
                    let end = (start + per_row).min(self.inputs.len());
                    ui.horizontal(|ui| {
                        for index in start..end {
                            ui.checkbox(&mut self.inputs[index], format!("Bit {}", index + 1));
                        }
                    });
                }
            });
    }

    fn show_outputs(&self, ui: &mut egui::Ui) {
        let per_row = (ui.available_width() / 10.0).clamp(1.0, 80.0) as usize;
        let rows = self.outputs.len().div_ceil(per_row);
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::vertical()
            .id_salt("output-bits")
            .max_height(144.0)
            .show_rows(ui, height, rows, |ui, visible| {
                for row in visible {
                    let start = row * per_row;
                    let end = (start + per_row).min(self.outputs.len());
                    // VM が返した ASCII の 0 と 1 を、表示幅に合わせて分ける。
                    ui.monospace(&self.outputs[start..end]);
                }
            });
    }

    fn step(&mut self) -> Result<(), String> {
        let tick = self.tick.checked_add(1).ok_or("Tick counter is full.")?;
        let frame: String = self
            .inputs
            .iter()
            .map(|&on| if on { '1' } else { '0' })
            .collect();
        self.outputs = self.vm.tick(&frame)?;
        self.tick = tick;
        Ok(())
    }
}

impl eframe::App for Workbench {
    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Neknaj Circuit Game 2");
                ui.separator();

                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Open project folder…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_parent(_frame).pick_folder()
                {
                    match crate::native::open_folder(&folder) {
                        Ok(project) => self.open_project(project),
                        Err(error) => self.error = Some(error.into()),
                    }
                }
                self.show_sources(ui);
                if ui.button("Load circuit").clicked() {
                    self.load_source();
                }
                self.show_error(ui);

                let Some(circuit) = &mut self.simulation else {
                    return;
                };
                ui.separator();
                ui.label(format!("Tick {}", circuit.tick));
                ui.label("Inputs");
                circuit.show_inputs(ui);
                ui.label("Outputs");
                circuit.show_outputs(ui);
                ui.horizontal(|ui| {
                    if ui.button("Reset").clicked() {
                        circuit.reset();
                        self.error = None;
                    }
                    if ui.button("Step").clicked() {
                        self.error = circuit.step().err().map(CompileError::from);
                    }
                });
            });
        });
    }
}

fn selection_for_bytes(source: &str, bytes: Range<usize>) -> Option<CCursorRange> {
    // Core はバイト位置、egui は先頭から数えた文字位置を使う。
    let selected = source.get(bytes.clone())?;
    let start = source.get(..bytes.start)?.chars().count();
    let end = start + selected.chars().count();
    Some(CCursorRange::two(CCursor::new(start), CCursor::new(end)))
}

#[cfg(test)]
mod tests {
    use super::selection_for_bytes;

    #[test]
    fn diagnostic_selection_uses_characters_and_keeps_empty_ranges() {
        let source = "漢🧪\n)";
        let offset = "漢🧪\n".len();
        let range = selection_for_bytes(source, offset..offset + 1)
            .unwrap()
            .as_sorted_char_range();
        assert_eq!(range.start.0..range.end.0, 3..4);

        let end = source.len();
        let insertion = selection_for_bytes(source, end..end)
            .unwrap()
            .as_sorted_char_range();
        assert_eq!(insertion.start.0..insertion.end.0, 4..4);
        assert!(selection_for_bytes(source, 1..3).is_none());
        assert!(selection_for_bytes(source, end..end + 1).is_none());
    }
}
