//! 選んだフォルダーから実行設定とソースファイルを読み込む。

use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

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
        settings: settings.into(),
        paths,
        texts,
    })
}

/// 期待バイト数を確かめてから、選択したスナップショットを読み込む。
///
/// # Errors
///
/// 通常のファイルを開けない場合、長さが異なる場合、領域を確保できない場合に失敗する。
pub(crate) fn read_snapshot(path: &Path, expected: usize) -> Result<Vec<u8>, String> {
    let io_error = |error| format!("{} を読み込めない。{error}", path.display());
    if !fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(format!("{} は通常のファイルではない。", path.display()));
    }
    let mut file = fs::File::open(path).map_err(io_error)?;
    let details = file.metadata().map_err(io_error)?;
    let wrong_len = || format!("{} の長さが現在の回路と一致しない。", path.display());
    if !details.is_file() || details.len() != expected as u64 {
        return Err(wrong_len());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| "ファイルを読む領域を確保できない。")?;
    bytes.resize(expected, 0);
    file.read_exact(&mut bytes).map_err(io_error)?;
    if file.read(&mut [0]).map_err(io_error)? != 0 {
        return Err(wrong_len());
    }
    Ok(bytes)
}

/// 全バイトを書き終えた一時ファイルを選択先に置く。
///
/// # Errors
///
/// 選択先と同じフォルダーで一時ファイルを作成、書き込み、配置できない場合に失敗する。
pub(crate) fn save_snapshot(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let io_error = |error| format!("{} に保存できない。{error}", path.display());
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.as_file().sync_all().map_err(io_error)?;
    file.persist(path).map_err(|error| io_error(error.error))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{MANIFEST, fs, open_folder};

    #[test]
    fn snapshot_storage_checks_expected_length_and_replaces_complete_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("状態.ncg2state");
        super::save_snapshot(&path, b"first").unwrap();
        assert_eq!(super::read_snapshot(&path, 5).unwrap(), b"first");
        assert!(super::read_snapshot(&path, 4).is_err());
        super::save_snapshot(&path, b"updated").unwrap();
        assert_eq!(super::read_snapshot(&path, 7).unwrap(), b"updated");
    }

    #[test]
    fn native_folder_resolves_sources_and_preserves_utf8_diagnostics() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("回路");
        let source = folder.join("src/main.ncg2");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(
            folder.join(MANIFEST),
            "sources = ['basis.ncg2', 'src/main.ncg2']\nentry = '::demo::gate'\n",
        )
        .unwrap();
        fs::write(folder.join("basis.ncg2"), "(using nor)").unwrap();
        fs::write(
            &source,
            "; 入出力\n(namespace demo (export gate) (module gate (in x) (out q) (def q (nor x x))))",
        )
        .unwrap();

        let project = open_folder(&folder).unwrap();
        assert_eq!(project.paths, ["basis.ncg2", "src/main.ncg2"]);
        let mut vm = project.compile().unwrap();
        assert_eq!(vm.reset(), "0");
        assert_eq!(vm.tick("0").unwrap(), "1");
        assert_eq!(vm.tick("1").unwrap(), "0");

        fs::write(&source, [b';', 0xff]).unwrap();
        assert_eq!(
            open_folder(&folder).err().unwrap(),
            "src/main.ncg2[1..2] 不正な UTF-8 が含まれる。",
        );
    }
}
