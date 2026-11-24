const manifest = "circuitgame2.toml";

export async function readProject(files, ProjectSettings, decodeUtf8, isCurrent) {
  const byPath = new Map(files.map((file) => {
    const relative = file.webkitRelativePath;
    const slash = relative.indexOf("/");
    return [slash < 0 ? "" : relative.slice(slash + 1), file];
  }));
  const settingsFile = byPath.get(manifest);
  if (!settingsFile) throw new Error(`選んだフォルダーの直下に ${manifest} がない。`);
  const settingsBytes = new Uint8Array(await settingsFile.arrayBuffer());
  if (!isCurrent()) return;
  const settings = new ProjectSettings(settingsBytes);
  try {
    const texts = [];
    for (const [index, path] of settings.source_paths().entries()) {
      const file = byPath.get(path);
      if (!file) throw new Error(settings.source_read_error(index));
      let bytes;
      try {
        bytes = new Uint8Array(await file.arrayBuffer());
      } catch {
        throw new Error(settings.source_read_error(index));
      }
      if (!isCurrent()) return;
      texts.push(decodeUtf8(path, bytes));
    }
    return settings.compile(texts);
  } finally {
    settings.free();
  }
}
