let workbench;
try {
  const { default: init, BrowserWorkbench } = await import("./pkg/circuitgame2_gui.js");
  await init();
  workbench = new BrowserWorkbench();
  await workbench.start(document.querySelector("#workbench"));
  const folder = document.querySelector("#folder");
  const projectStatus = document.querySelector("#project-status");
  let pending;
  folder.addEventListener("change", async () => {
    const files = new Map();
    for (const file of folder.files) {
      const relative = file.webkitRelativePath;
      const slash = relative.indexOf("/");
      if (slash >= 0) files.set(relative.slice(slash + 1), file);
    }
    folder.value = "";

    pending?.abort();
    pending = new AbortController();
    const { signal } = pending;
    projectStatus.textContent = "Opening project…";
    try {
      await workbench.open_project(files, signal);
      if (!signal.aborted) projectStatus.textContent = "";
    } catch (error) {
      if (!signal.aborted) {
        projectStatus.textContent = error instanceof Error ? error.message : String(error);
      }
    }
  });
  document.querySelector("#project-controls").hidden = false;
} catch (error) {
  document.querySelector("#workbench").hidden = true;
  const status = document.querySelector("#status");
  status.textContent = error instanceof Error ? error.message : String(error);
  status.hidden = false;
}
