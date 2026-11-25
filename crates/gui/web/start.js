let workbench;
try {
  const { default: init, BrowserWorkbench } = await import("./pkg/circuitgame2_gui.js");
  await init();
  workbench = new BrowserWorkbench();
  await workbench.start(document.querySelector("#workbench"));
} catch (error) {
  document.querySelector("#workbench").hidden = true;
  const status = document.querySelector("#status");
  status.textContent = error instanceof Error ? error.message : String(error);
  status.hidden = false;
}
