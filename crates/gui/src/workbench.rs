//! 回路本文と、その本文から作ったシミュレーターを保持する。

use circuitgame2_web::Simulation;
use eframe::egui::{self, RichText, TextEdit};

use crate::project::Project;

const EXAMPLE: &str = include_str!("../../../site/examples/main.ncg2");

/// 編集した NCG2 ソースを読み込み、入力と経過 tick を操作する画面。
pub struct Workbench {
    sources: Sources,
    selected: usize,
    project_generation: u64,
    simulation: Option<LoadedCircuit>,
    error: Option<String>,
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
            let editor = ui
                .add(
                    TextEdit::multiline(source)
                        // 開き直した別のプロジェクトに、元の編集履歴を引き継がない。
                        .id_salt(("source-editor", self.project_generation, self.selected))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(12),
                )
                .labelled_by(label.id);
            if editor.changed() {
                self.simulation = None;
                self.error = None;
            }
        }
    }

    fn load_source(&mut self) {
        self.simulation = None;
        self.error = None;
        let result = match &self.sources {
            Sources::Single(source) => Simulation::new(source),
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
                        Err(error) => self.error = Some(error),
                    }
                }
                self.show_sources(ui);
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
