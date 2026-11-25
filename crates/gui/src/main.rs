//! ネイティブのウィンドウで NCG2 の画面を開く。

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "Neknaj Circuit Game 2",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([900.0, 640.0]),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(circuitgame2_gui::Workbench::default()))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
