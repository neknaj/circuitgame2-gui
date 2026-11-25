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
        settings: settings.into(),
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

#[cfg(test)]
mod tests {
    use super::{MANIFEST, fs, open_folder};

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
