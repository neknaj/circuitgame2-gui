//! 編集できるプロジェクトの本文を、設定のソース順で保持する。

use std::sync::Arc;

use circuitgame2_web::ProjectSettings;
#[cfg(any(target_arch = "wasm32", test))]
use circuitgame2_web::{CompileError, Simulation};

pub(crate) use circuitgame2_web::PROJECT_CONFIG_FILE_NAME as MANIFEST;

#[derive(Clone)]
pub(crate) struct Project {
    pub(crate) settings: Arc<ProjectSettings>,
    pub(crate) paths: Vec<String>,
    pub(crate) texts: Vec<String>,
}

impl Project {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn compile(&self) -> Result<Simulation, CompileError> {
        self.settings.compile_texts(&self.texts)
    }
}
