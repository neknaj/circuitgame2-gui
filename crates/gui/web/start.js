import { listSnapshots, readSnapshot, saveSnapshot } from "./snapshot-store.js";

const errorMessage = (error) => error instanceof Error ? error.message : String(error);

try {
  const { default: init, BrowserWorkbench } = await import("./pkg/circuitgame2_gui.js");
  await init();
  const workbench = new BrowserWorkbench();
  await workbench.start(document.querySelector("#workbench"));
  const folder = document.querySelector("#folder");
  const exportSnapshot = document.querySelector("#export-snapshot");
  const importSnapshot = document.querySelector("#import-snapshot");
  const saveBrowserSnapshot = document.querySelector("#save-browser-snapshot");
  const savedSnapshots = document.querySelector("#saved-snapshots");
  const restoreBrowserSnapshot = document.querySelector("#restore-browser-snapshot");
  const projectStatus = document.querySelector("#project-status");
  let pending;
  let listVersion = 0;
  let operationVersion = 0;

  function updateControls() {
    const busy = pending !== undefined;
    exportSnapshot.disabled = busy;
    importSnapshot.disabled = busy;
    saveBrowserSnapshot.disabled = busy;
    savedSnapshots.disabled = busy;
    restoreBrowserSnapshot.disabled = busy || savedSnapshots.value === "";
  }

  async function refreshSnapshots(preferred, request) {
    const version = ++listVersion;
    const snapshots = await listSnapshots();
    if (version !== listVersion) return;
    const selected = request && request === pending ? preferred : savedSnapshots.value;
    const options = document.createDocumentFragment();
    options.append(new Option("Select a snapshot", ""));
    for (const { id, savedAt, size } of snapshots) {
      const label = new Date(savedAt).toLocaleString();
      options.append(new Option(`${label} (${size.toLocaleString()} bytes)`, String(id)));
    }
    savedSnapshots.replaceChildren(options);
    savedSnapshots.value = selected;
    if (savedSnapshots.value === "") savedSnapshots.selectedIndex = 0;
    updateControls();
  }

  function beginReading(message) {
    operationVersion += 1;
    pending?.abort();
    pending = new AbortController();
    updateControls();
    projectStatus.textContent = message;
    return pending;
  }

  function endReading(request) {
    if (request !== pending) return;
    pending = undefined;
    updateControls();
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
    operationVersion += 1;
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
      if (!request.signal.aborted) {
        projectStatus.textContent = restored ? "Snapshot restored." : "The circuit changed while reading.";
      }
    } catch (error) {
      if (!request.signal.aborted) projectStatus.textContent = errorMessage(error);
    } finally {
      endReading(request);
    }
  });

  savedSnapshots.addEventListener("change", updateControls);

  saveBrowserSnapshot.addEventListener("click", async () => {
    if (pending) return;
    const request = beginReading("Saving snapshot…");
    try {
      const bytes = workbench.export_snapshot();
      const id = await saveSnapshot(bytes);
      if (!request.signal.aborted) projectStatus.textContent = "Snapshot saved in this browser.";
      try {
        await refreshSnapshots(String(id), request);
      } catch (error) {
        if (!request.signal.aborted) {
          projectStatus.textContent = `Snapshot saved. Could not read the list. ${errorMessage(error)}`;
        }
      }
    } catch (error) {
      if (!request.signal.aborted) projectStatus.textContent = errorMessage(error);
    } finally {
      endReading(request);
    }
  });

  restoreBrowserSnapshot.addEventListener("click", async () => {
    if (pending || savedSnapshots.value === "") return;
    const id = Number(savedSnapshots.value);
    const request = beginReading("Reading saved snapshot…");
    try {
      const version = workbench.snapshot_context();
      const file = await readSnapshot(id);
      if (request.signal.aborted) return;
      const sameCircuit = workbench.snapshot_context() === version;
      const restored = sameCircuit && await workbench.import_snapshot(
        file,
        request.signal,
      );
      if (!request.signal.aborted) {
        projectStatus.textContent = restored ? "Snapshot restored." : "The circuit changed while reading.";
      }
    } catch (error) {
      if (!request.signal.aborted) projectStatus.textContent = errorMessage(error);
    } finally {
      endReading(request);
    }
  });

  document.querySelector("#project-controls").hidden = false;
  const initialOperation = operationVersion;
  refreshSnapshots().catch((error) => {
    if (operationVersion === initialOperation && !pending) {
      projectStatus.textContent = errorMessage(error);
    }
  });
} catch (error) {
  document.querySelector("#workbench").hidden = true;
  const status = document.querySelector("#status");
  status.textContent = errorMessage(error);
  status.hidden = false;
}
