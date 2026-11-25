//! 編集できるプロジェクトの本文を、設定のソース順で保持する。

use circuitgame2_web::{CompileError, ProjectSettings, Simulation};

pub(crate) const MANIFEST: &str = "circuitgame2.toml";

pub(crate) struct Project {
    pub(crate) settings: ProjectSettings,
    pub(crate) paths: Vec<String>,
    pub(crate) texts: Vec<String>,
}

impl Project {
    pub(crate) fn compile(&self) -> Result<Simulation, CompileError> {
        self.settings.compile_texts(&self.texts)
    }
}
