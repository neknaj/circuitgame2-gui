//! ネイティブとブラウザで同じ回路の編集と tick 操作を提供する。

#[cfg(not(target_arch = "wasm32"))]
mod native;
mod project;
#[cfg(target_arch = "wasm32")]
mod web;
mod workbench;

pub use workbench::Workbench;
