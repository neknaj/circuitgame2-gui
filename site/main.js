const source = document.querySelector("#source");
const compile = document.querySelector("#compile");
const reset = document.querySelector("#reset");
const tick = document.querySelector("#tick");
const status = document.querySelector("#status");
const inputs = document.querySelector("#inputs");
const outputs = document.querySelector("#outputs");
const position = document.querySelector("#position");

let Simulation;
let simulation;
let tickNumber = 0n;
let switches = [];
let sourceEdited = false;

function clearSimulation() {
  simulation?.free();
  simulation = undefined;
  switches = [];
  inputs.replaceChildren();
  outputs.replaceChildren();
  reset.disabled = true;
  tick.disabled = true;
  position.textContent = "回路を読み込むと tick 番号を表示する。";
}

function showOutputs(bits) {
  const elements = Array.from(bits, (value) => {
    const led = document.createElement("span");
    led.className = "led";
    led.dataset.value = value;
    led.textContent = value;
    return led;
  });
  outputs.replaceChildren(...elements);
  position.textContent = bits
    ? `tick ${tickNumber}、出力 ${bits}`
    : `tick ${tickNumber}、出力なし`;
}

function resetSimulation() {
  const bits = simulation.reset();
  tickNumber = 0n;
  for (const input of switches) input.checked = false;
  showOutputs(bits);
  status.textContent = "";
}

function loadSource() {
  clearSimulation();
  try {
    simulation = new Simulation(source.value);
    const width = simulation.input_width();
    if (width > 256 || simulation.output_width() > 256) {
      throw new Error("この画面で表示できるのは、入力・出力それぞれ 256 ビットまで。");
    }
    for (let index = 0; index < width; index += 1) {
      const label = document.createElement("label");
      const input = document.createElement("input");
      input.type = "checkbox";
      label.append(input, `ビット ${index + 1}`);
      inputs.append(label);
      switches.push(input);
    }
    resetSimulation();
    reset.disabled = false;
    tick.disabled = false;
  } catch (error) {
    clearSimulation();
    status.textContent = error instanceof Error ? error.message : String(error);
  }
}

source.addEventListener("input", () => {
  sourceEdited = true;
  clearSimulation();
  status.textContent = "編集中のソースで実行するには、回路を読み込む。";
});
compile.addEventListener("click", loadSource);
reset.addEventListener("click", resetSimulation);
tick.addEventListener("click", () => {
  try {
    const frame = switches.map((input) => input.checked ? "1" : "0").join("");
    const bits = simulation.tick(frame);
    tickNumber += 1n;
    showOutputs(bits);
    status.textContent = "";
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
  }
});

try {
  const { default: init, Simulation: CircuitSimulation } = await import("./pkg/circuitgame2_web.js");
  await init();
  Simulation = CircuitSimulation;
  compile.disabled = false;
  const response = await fetch("./examples/main.ncg2");
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const example = await response.text();
  if (!sourceEdited && !source.value) {
    source.value = example;
    loadSource();
  }
} catch (error) {
  const subject = Simulation ? "例題" : "シミュレーター";
  status.textContent = `${subject}を読み込めない。 ${error}`;
}
