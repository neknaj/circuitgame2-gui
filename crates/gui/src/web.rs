//! ブラウザの Canvas に共通の画面を接続する。

use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};
use eframe::{WebOptions, WebRunner};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::Workbench;

const JAPANESE_FONT: &[u8] = include_bytes!("../web/fonts/MPLUS1p-Regular.ttf");

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

    /// 指定した Canvas で画面を開き、カナや漢字を表示するためのフォントを登録する。
    ///
    /// # Errors
    ///
    /// 描画の初期化に失敗した場合にブラウザーのエラーを返す。
    pub async fn start(&self, canvas: HtmlCanvasElement) -> Result<(), JsValue> {
        self.runner
            .start(
                canvas,
                WebOptions::default(),
                Box::new(|context| {
                    let mut fonts = FontDefinitions::default();
                    let name = "M PLUS 1p";
                    fonts.font_data.insert(
                        name.to_owned(),
                        Arc::new(FontData::from_static(JAPANESE_FONT)),
                    );

                    // 既定のフォントで描けない文字を、ソース欄と診断の両方で補う。
                    for family in [FontFamily::Monospace, FontFamily::Proportional] {
                        fonts
                            .families
                            .entry(family)
                            .or_default()
                            .push(name.to_owned());
                    }
                    context.egui_ctx.set_fonts(fonts);
                    Ok(Box::new(Workbench::default()))
                }),
            )
            .await
    }
}
