//! 選んだフォルダーから実行設定とソースファイルを読み込む。

use std::{fs, path::Path};

use circuitgame2_web::{ProjectSettings, decode_utf8};

use crate::project::{MANIFEST, Project};

pub(crate) fn open_folder(folder: &Path) -> Result<Project, String> {
    let folder = folder
        .canonicalize()
        .map_err(|error| format!("{} を開けない。{error}", folder.display()))?;
    let settings_file = read_inside(&folder, MANIFEST)
        .map_err(|error| format!("{MANIFEST} を読み込めない。{error}"))?;
    let settings = ProjectSettings::new(&settings_file)?;
    let paths = settings.source_paths();
    let mut texts = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let bytes = read_inside(&folder, path)
            .map_err(|error| format!("{} {error}", settings.source_read_error(index)))?;
        texts.push(decode_utf8(path, &bytes)?);
    }

    Ok(Project {
        settings,
        paths,
        texts,
    })
}

fn read_inside(folder: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = folder
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    // リンク先も確認し、選択したフォルダーの外にあるファイルは読み込まない。
    if !path.starts_with(folder) {
        return Err("選択したフォルダーの外を参照している。".to_owned());
    }
    fs::read(path).map_err(|error| error.to_string())
}
