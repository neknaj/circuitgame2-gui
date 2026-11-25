//! 回路本文と、その本文から作ったシミュレーターを保持する。

use circuitgame2_web::Simulation;
use eframe::egui::{self, RichText, TextEdit};

const EXAMPLE: &str = include_str!("../../../site/examples/main.ncg2");

/// 編集した NCG2 ソースを読み込み、入力と経過 tick を操作する画面。
pub struct Workbench {
    source: String,
    simulation: Option<LoadedCircuit>,
    error: Option<String>,
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
            source: EXAMPLE.to_owned(),
            simulation: None,
            error: None,
        };
        app.load_source();
        app
    }
}

impl Workbench {
    fn load_source(&mut self) {
        self.simulation = None;
        self.error = None;
        match Simulation::new(&self.source) {
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Neknaj Circuit Game 2");
                ui.separator();

                let label = ui.label("Source");
                let source = ui
                    .add(
                        TextEdit::multiline(&mut self.source)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(12),
                    )
                    .labelled_by(label.id);
                if source.changed() {
                    self.simulation = None;
                    self.error = None;
                }
                if ui.button("Load circuit").clicked() {
                    self.load_source();
                }
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(ui.visuals().error_fg_color));
                }

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
                        self.error = circuit.step().err();
                    }
                });
            });
        });
    }
}
