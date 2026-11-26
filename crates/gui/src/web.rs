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
        self.runner
            .app_mut::<Workbench>()
            .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?
            .open_project(project);
        self.repaint();
        Ok(())
    }

    /// 現在の確定済み tick を、CLI と共通のバイナリ形式で返す。
    ///
    /// # Errors
    ///
    /// 回路が読み込まれていない場合、実行構成が保存に対応していない場合、または符号化に失敗した場合に返す。
    pub fn export_snapshot(&self) -> Result<Uint8Array, JsValue> {
        let workbench = self
            .runner
            .app_mut::<Workbench>()
            .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?;
        let bytes = workbench
            .create_snapshot()
            .map_err(|error| JsValue::from_str(&error))?;
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    /// 読み込みの開始時と終了時に照合する回路の世代を返す。
    ///
    /// # Errors
    ///
    /// 画面が閉じている場合に失敗する。
    pub fn snapshot_context(&self) -> Result<u64, JsValue> {
        let workbench = self
            .runner
            .app_mut::<Workbench>()
            .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?;
        Ok(workbench.snapshot_context())
    }

    /// 選んだファイルから、読み込みを始めたときの回路に実行状態を戻す。
    ///
    /// 回路から求めたサイズとファイルのサイズを照合してから読み込む。中断を通知された場合は `false` を返す。
    ///
    /// # Errors
    ///
    /// ファイルの長さや内容が回路に合わない場合、読み込み中に対象の回路が変わった場合、
    /// またはファイルを読めない場合に返す。
    pub async fn import_snapshot(&self, file: File, signal: AbortSignal) -> Result<bool, JsValue> {
        if signal.aborted() {
            return Ok(false);
        }
        let (expected, snapshot_context) = {
            let workbench = self
                .runner
                .app_mut::<Workbench>()
                .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?;
            let expected = workbench
                .snapshot_len()
                .map_err(|error| JsValue::from_str(&error))?;
            (expected, workbench.snapshot_context())
        };
        let wrong_size = || {
            JsValue::from_str(&format!(
                "スナップショットのサイズが必要な {expected} バイトと異なる。"
            ))
        };
        if file.size() != expected as f64 {
            return Err(wrong_size());
        }

        let buffer = JsFuture::from(file.array_buffer()).await;
        if signal.aborted() {
            return Ok(false);
        }
        let buffer = buffer.map_err(|_| JsValue::from_str("スナップショットを読み込めない。"))?;
        let bytes = Uint8Array::new(&buffer);
        if bytes.length() as usize != expected {
            return Err(wrong_size());
        }
        {
            let mut workbench = self
                .runner
                .app_mut::<Workbench>()
                .ok_or_else(|| JsValue::from_str("回路の画面を開いていない。"))?;
            if workbench.snapshot_context() != snapshot_context {
                return Err(JsValue::from_str("読み込み中に回路が切り替わった。"));
            }
            workbench
                .restore_snapshot(&bytes.to_vec())
                .map_err(|error| JsValue::from_str(&error))?;
        }
        self.repaint();
        Ok(true)
    }

    fn repaint(&self) {
        if let Some(context) = self.context.borrow().as_ref() {
            context.request_repaint();
        }
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
