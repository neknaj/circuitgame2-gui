//! ブラウザの Canvas に共通の画面を接続する。

use eframe::{WebOptions, WebRunner};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::Workbench;

/// 開いている Canvas と、描画を続けるための実行状態。
#[wasm_bindgen]
pub struct BrowserWorkbench {
    runner: WebRunner,
}

#[wasm_bindgen]
impl BrowserWorkbench {
    /// ブラウザ用の実行状態を作る。
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            runner: WebRunner::new(),
        }
    }

    /// 指定した Canvas で画面を開く。
    ///
    /// # Errors
    ///
    /// 描画の初期化に失敗した場合にブラウザのエラーを返す。
    pub async fn start(&self, canvas: HtmlCanvasElement) -> Result<(), JsValue> {
        self.runner
            .start(
                canvas,
                WebOptions::default(),
                Box::new(|_| Ok(Box::new(Workbench::default()))),
            )
            .await
    }
}
