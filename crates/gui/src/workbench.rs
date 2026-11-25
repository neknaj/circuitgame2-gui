//! 回路本文と、その本文から作ったシミュレーターを保持する。

use std::{ops::Range, time::Duration};

use circuitgame2_web::{CompileError, Simulation};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, RichText, TextEdit};

use crate::{project::Project, vram::VramPanel, waveform::Waveform};

const EXAMPLE: &str = include_str!("../../../site/examples/main.ncg2");
const MAX_TICKS_PER_FRAME: u32 = 8;

struct Playback {
    speed: u32,
    next_tick: Option<f64>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            speed: 10,
            next_tick: None,
        }
    }
}

impl Playback {
    fn pause(&mut self) {
        self.next_tick = None;
    }

    fn due(&mut self, now: f64) -> u32 {
        let Some(deadline) = self.next_tick.filter(|&deadline| deadline <= now) else {
            return 0;
        };
        let speed = f64::from(self.speed);
        // 長く描画が止まった後も、画面を固めずに入力を操作できるようにする。
        let ticks = (((now - deadline) * speed) as u32)
            .saturating_add(1)
            .min(MAX_TICKS_PER_FRAME);
        let next = deadline + f64::from(ticks) / speed;
        self.next_tick = Some(if next > now { next } else { now + 1.0 / speed });
        ticks
    }
}

/// 編集した NCG2 ソースを読み込み、入力と経過 tick を操作する画面。
pub struct Workbench {
    sources: Sources,
    selected: usize,
    project_generation: u64,
    simulation: Option<LoadedCircuit>,
    playback: Playback,
    error: Option<CompileError>,
    pending_jump: Option<(usize, Range<usize>)>,
    #[cfg(not(target_arch = "wasm32"))]
    gpu_batch: crate::gpu_batch::GpuBatch,
    #[cfg(not(target_arch = "wasm32"))]
    function_entry: bool,
}

#[derive(Clone)]
enum Sources {
    Single(String),
    Project(Project),
}

struct LoadedCircuit {
    vm: Simulation,
    inputs: Vec<bool>,
    outputs: String,
    tick: u64,
    vram: VramPanel,
    waveform: Waveform,
}

impl Default for Workbench {
    fn default() -> Self {
        let mut app = Self {
            sources: Sources::Single(EXAMPLE.to_owned()),
            selected: 0,
            project_generation: 0,
            simulation: None,
            playback: Playback::default(),
            error: None,
            pending_jump: None,
            #[cfg(not(target_arch = "wasm32"))]
            gpu_batch: crate::gpu_batch::GpuBatch::default(),
            #[cfg(not(target_arch = "wasm32"))]
            function_entry: false,
        };
        app.load_source();
        app
    }
}

impl Workbench {
    pub(crate) fn open_project(&mut self, project: Project) {
        self.sources = Sources::Project(project);
        #[cfg(not(target_arch = "wasm32"))]
        self.gpu_batch.invalidate();
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
                        self.playback.pause();
                        self.error = None;
                        self.pending_jump = None;
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.gpu_batch.invalidate();
                            self.function_entry = false;
                        }
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

    fn load_source(&mut self) {
        self.playback.pause();
        self.simulation = None;
        self.error = None;
        self.pending_jump = None;
        #[cfg(not(target_arch = "wasm32"))]
        let result = {
            self.function_entry = false;
            match &self.sources {
                Sources::Single(source) => Simulation::compile_workbench(source),
                Sources::Project(project) => {
                    project.settings.compile_workbench_texts(&project.texts)
                }
            }
        };
        #[cfg(target_arch = "wasm32")]
        let result = match &self.sources {
            Sources::Single(source) => Simulation::compile(source),
            Sources::Project(project) => project.compile(),
        }
        .map(Some);
        match result {
            Ok(None) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.function_entry = true;
                }
            }
            Ok(Some(mut vm)) => {
                let inputs = vec![false; vm.input_width()];
                let outputs = vm.reset();
                let vram = VramPanel::new(&vm);
                let mut waveform = Waveform::new(vm.input_width(), vm.output_width());
                waveform.record(&inputs, &outputs);
                self.simulation = Some(LoadedCircuit {
                    vm,
                    inputs,
                    outputs,
                    tick: 0,
                    vram,
                    waveform,
                });
            }
            Err(error) => self.error = Some(error),
        }
    }
}

enum PlaybackAction {
    None,
    Step,
    Reset,
}

impl Workbench {
    fn show_playback(&mut self, ui: &mut egui::Ui) -> PlaybackAction {
        if self.simulation.is_none() {
            return PlaybackAction::None;
        }
        let now = ui.input(|input| input.time);
        ui.horizontal_wrapped(|ui| {
            let running = self.playback.next_tick.is_some();
            if ui.button(if running { "Pause" } else { "Run" }).clicked() {
                self.playback.next_tick = (!running).then_some(now);
                if !running {
                    self.error = None;
                }
            }
            let step = ui
                .add_enabled(self.playback.next_tick.is_none(), egui::Button::new("Step"))
                .clicked();
            let reset = ui.button("Reset").clicked();
            let speed_changed = ui
                .add(
                    egui::Slider::new(&mut self.playback.speed, 1..=240)
                        .logarithmic(true)
                        .suffix(" ticks/s"),
                )
                .changed();
            if speed_changed && self.playback.next_tick.is_some() {
                self.playback.next_tick = Some(now + 1.0 / f64::from(self.playback.speed));
            }
            if reset {
                PlaybackAction::Reset
            } else if step {
                PlaybackAction::Step
            } else {
                PlaybackAction::None
            }
        })
        .inner
    }

    fn advance_playback(&mut self, ctx: &egui::Context, action: PlaybackAction) {
        let Some(circuit) = self.simulation.as_mut() else {
            return;
        };
        let now = ctx.input(|input| input.time);
        let count = match action {
            PlaybackAction::Reset => {
                self.playback.pause();
                self.error = None;
                circuit.reset();
                ctx.request_repaint();
                return;
            }
            PlaybackAction::Step => {
                self.error = None;
                1
            }
            PlaybackAction::None => self.playback.due(now),
        };
        for _ in 0..count {
            if let Err(error) = circuit.step() {
                self.playback.pause();
                self.error = Some(error.into());
                break;
            }
        }
        if let Some(next) = self.playback.next_tick {
            ctx.request_repaint_after(Duration::from_secs_f64((next - now).max(0.0)));
        }
    }
}

impl LoadedCircuit {
    fn reset(&mut self) {
        self.inputs.fill(false);
        self.outputs = self.vm.reset();
        self.tick = 0;
        self.waveform.reset(&self.inputs, &self.outputs);
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
        self.waveform.record(&self.inputs, &self.outputs);
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
            ui.heading("Neknaj Circuit Game 2");
            let mut playback_action = self.show_playback(ui);
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Open project folder…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_parent(_frame).pick_folder()
                {
                    match crate::native::open_folder(&folder) {
                        Ok(project) => {
                            self.open_project(project);
                            playback_action = PlaybackAction::None;
                        }
                        Err(error) => self.error = Some(error.into()),
                    }
                }
                self.show_sources(ui);
                if ui.button("Load circuit").clicked() {
                    self.load_source();
                    playback_action = PlaybackAction::None;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.function_entry {
                        ui.label("Use GPU batch to evaluate this func.");
                    }
                    self.gpu_batch.poll(ui.ctx());
                    if self.gpu_batch.show(ui) {
                        let sources = self.sources.clone();
                        self.gpu_batch
                            .start(ui.ctx().clone(), move || match sources {
                                Sources::Single(source) => {
                                    circuitgame2_web::compile_gpu_batch_source(&source)
                                }
                                Sources::Project(project) => {
                                    project.settings.compile_gpu_batch_texts(&project.texts)
                                }
                            });
                    }
                    if let Some(error) = self.gpu_batch.error() {
                        show_diagnostic(
                            ui,
                            error,
                            &self.sources,
                            &mut self.selected,
                            &mut self.pending_jump,
                        );
                    }
                }
                if let Some(circuit) = &mut self.simulation {
                    ui.separator();
                    ui.label("Inputs");
                    circuit.show_inputs(ui);
                }
                // 同じ描画フレームの入力操作を、次に確定する tick に反映する。
                self.advance_playback(ui.ctx(), playback_action);
                if let Some(error) = &self.error {
                    show_diagnostic(
                        ui,
                        error,
                        &self.sources,
                        &mut self.selected,
                        &mut self.pending_jump,
                    );
                }

                let Some(circuit) = &mut self.simulation else {
                    return;
                };
                ui.label(format!("Tick {}", circuit.tick));
                ui.label("Outputs");
                circuit.show_outputs(ui);
                circuit.vram.show(ui, &circuit.vm);
                circuit.waveform.show(ui, circuit.tick);
            });
        });
    }
}

fn show_diagnostic(
    ui: &mut egui::Ui,
    error: &CompileError,
    sources: &Sources,
    selected: &mut usize,
    pending_jump: &mut Option<(usize, Range<usize>)>,
) {
    ui.label(RichText::new(error.to_string()).color(ui.visuals().error_fg_color));
    let Some((path, span)) = error.source_location() else {
        return;
    };
    let file_index = |path: &str| match sources {
        Sources::Single(_) => Some(0),
        Sources::Project(project) => project.paths.iter().position(|candidate| candidate == path),
    };
    let ctx = ui.ctx().clone();
    let mut jump = |file, span: &Range<usize>| {
        *selected = file;
        *pending_jump = Some((file, span.clone()));
        ctx.request_repaint();
    };
    if let Some(file) = file_index(path)
        && ui.button("Go to error").clicked()
    {
        jump(file, span);
    }
    for location in error.related_source_locations() {
        let range = &location.span;
        if let Some(file) = file_index(&location.path)
            && ui
                .button(format!(
                    "{} [{}..{}]",
                    location.path, range.start, range.end
                ))
                .clicked()
        {
            jump(file, range);
        }
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
    use super::{MAX_TICKS_PER_FRAME, Playback, selection_for_bytes};

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn workbench_opens_a_function_without_a_module_diagnostic() {
        let mut workbench = super::Workbench::default();
        let source = "(using nor) (func main (in x) (out q) (def q (nor x x)))";
        workbench.sources = super::Sources::Single(source.into());
        workbench.load_source();

        assert!(workbench.function_entry);
        assert!(workbench.simulation.is_none());
        assert!(workbench.error.is_none());
    }

    #[test]
    fn playback_limits_work_after_a_long_frame_and_does_not_replay_it_after_pause() {
        let mut playback = Playback {
            speed: 4,
            next_tick: Some(1.0),
        };
        assert_eq!(playback.due(0.75), 0);
        assert_eq!(playback.due(1.0), 1);
        assert_eq!(playback.due(1.50), 2);
        assert_eq!(playback.due(30.0), MAX_TICKS_PER_FRAME);
        assert_eq!(playback.due(30.0), 0);
        playback.pause();
        assert_eq!(playback.due(31.0), 0);
        playback.next_tick = Some(31.0);
        assert_eq!(playback.due(31.0), 1);
    }

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
