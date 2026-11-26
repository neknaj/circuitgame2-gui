const errorMessage = (error) => error instanceof Error ? error.message : String(error);

try {
  const { default: init, BrowserWorkbench } = await import("./pkg/circuitgame2_gui.js");
  await init();
  const workbench = new BrowserWorkbench();
  await workbench.start(document.querySelector("#workbench"));
  const folder = document.querySelector("#folder");
  const exportSnapshot = document.querySelector("#export-snapshot");
  const importSnapshot = document.querySelector("#import-snapshot");
  const projectStatus = document.querySelector("#project-status");
  let pending;

  function beginReading(message) {
    pending?.abort();
    pending = new AbortController();
    exportSnapshot.disabled = true;
    importSnapshot.disabled = true;
    projectStatus.textContent = message;
    return pending;
  }

  function endReading(request) {
    if (request !== pending) return;
    pending = undefined;
    exportSnapshot.disabled = false;
    importSnapshot.disabled = false;
  }

  folder.addEventListener("change", async () => {
    const files = new Map();
    for (const file of folder.files) {
      const relative = file.webkitRelativePath;
      const slash = relative.indexOf("/");
      if (slash >= 0) files.set(relative.slice(slash + 1), file);
    }
    folder.value = "";

    const request = beginReading("Opening project…");
    try {
      await workbench.open_project(files, request.signal);
      if (!request.signal.aborted) projectStatus.textContent = "";
    } catch (error) {
      if (!request.signal.aborted) projectStatus.textContent = errorMessage(error);
    } finally {
      endReading(request);
    }
  });

  exportSnapshot.addEventListener("click", () => {
    if (pending) return;
    try {
      const bytes = workbench.export_snapshot();
      const blob = new Blob([bytes], { type: "application/octet-stream" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = "ncg2-snapshot.bin";
      link.hidden = true;
      document.body.append(link);
      try {
        link.click();
        projectStatus.textContent = "Snapshot download requested.";
      } finally {
        link.remove();
        // ダウンロードの要求直後に失効しないよう、URL を短時間保つ。
        setTimeout(() => URL.revokeObjectURL(url), 60_000);
      }
    } catch (error) {
      projectStatus.textContent = errorMessage(error);
    }
  });

  importSnapshot.addEventListener("change", async () => {
    const file = importSnapshot.files[0];
    importSnapshot.value = "";
    if (!file || pending) return;
    const request = beginReading("Reading snapshot…");
    try {
      const restored = await workbench.import_snapshot(file, request.signal);
      if (!request.signal.aborted && restored) projectStatus.textContent = "Snapshot restored.";
    } catch (error) {
      if (!request.signal.aborted) projectStatus.textContent = errorMessage(error);
    } finally {
      endReading(request);
    }
  });

  document.querySelector("#project-controls").hidden = false;
} catch (error) {
  document.querySelector("#workbench").hidden = true;
  const status = document.querySelector("#status");
  status.textContent = errorMessage(error);
  status.hidden = false;
}
