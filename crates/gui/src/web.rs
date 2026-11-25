//! ブラウザの Canvas に共通の画面を接続する。

use std::{cell::RefCell, rc::Rc, sync::Arc};

use circuitgame2_web::{ProjectSettings, decode_utf8};

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};
use eframe::{WebOptions, WebRunner};
use js_sys::{Map, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortSignal, File, HtmlCanvasElement};

use crate::{
    Workbench,
    project::{MANIFEST, Project},
};

const JAPANESE_FONT: &[u8] = include_bytes!("../web/fonts/MPLUS1p-Regular.ttf");

/// 開いている Canvas と、描画を続けるための実行状態。
#[wasm_bindgen]
pub struct BrowserWorkbench {
    runner: WebRunner,
    context: Rc<RefCell<Option<Context>>>,
}

#[wasm_bindgen]
impl BrowserWorkbench {
    /// ブラウザ用の実行状態を作る。
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            runner: WebRunner::new(),
            context: Rc::default(),
        }
    }

    /// 指定した Canvas で画面を開き、カナや漢字を表示するためのフォントを登録する。
    ///
    /// # Errors
    ///
    /// 描画の初期化に失敗した場合にブラウザーのエラーを返す。
    pub async fn start(&self, canvas: HtmlCanvasElement) -> Result<(), JsValue> {
        let repaint_context = self.context.clone();
        self.runner
            .start(
                canvas,
                WebOptions::default(),
                Box::new(move |context| {
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
                    *repaint_context.borrow_mut() = Some(context.egui_ctx.clone());
                    Ok(Box::new(Workbench::default()))
                }),
            )
            .await
    }

    /// フォルダー内のファイルを読み、設定に従って編集対象を切り替える。
    ///
    /// `files` のキーは、選択フォルダーからの相対パス。新しい選択で `signal`
    /// が中断された場合は、それまでの画面を保つ。
    ///
    /// # Errors
    ///
    /// ファイルを開けない場合、設定を読み取れない場合、または画面が閉じている場合に失敗する。
    pub async fn open_project(&self, files: Map, signal: AbortSignal) -> Result<(), JsValue> {
        let project = read_project(files, &signal).await;
        if signal.aborted() {
            return Ok(());
        }
        let Some(project) = project? else {
            return Ok(());
        };
        let mut workbench = self
            .runner
            .app_mut::<Workbench>()
            .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?;
        workbench.open_project(project);
        if let Some(context) = self.context.borrow().as_ref() {
            context.request_repaint();
        }
        Ok(())
    }
}

async fn read_project(files: Map, signal: &AbortSignal) -> Result<Option<Project>, JsValue> {
    let settings = files
        .get(&JsValue::from_str(MANIFEST))
        .dyn_into::<File>()
        .map_err(|_| JsValue::from_str("選んだフォルダーの直下に circuitgame2.toml がない。"))?;
    let bytes = read_bytes(&settings)
        .await
        .map_err(|_| JsValue::from_str("circuitgame2.toml を読み込めない。"))?;
    if signal.aborted() {
        return Ok(None);
    }
    let settings = ProjectSettings::new(&bytes)?;
    let paths = settings.source_paths();
    let mut texts = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let file = files
            .get(&JsValue::from_str(path))
            .dyn_into::<File>()
            .map_err(|_| settings.source_read_error(index))?;
        let bytes = read_bytes(&file)
            .await
            .map_err(|_| settings.source_read_error(index))?;
        if signal.aborted() {
            return Ok(None);
        }
        texts.push(decode_utf8(path, &bytes)?);
    }

    Ok(Some(Project {
        settings: settings.into(),
        paths,
        texts,
    }))
}

async fn read_bytes(file: &File) -> Result<Vec<u8>, JsValue> {
    let buffer = JsFuture::from(file.array_buffer()).await?;
    Ok(Uint8Array::new(&buffer).to_vec())
}
