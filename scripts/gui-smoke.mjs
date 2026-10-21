// End-to-end check of the stock GUI: a headless browser uses the GUI like a person while an
// "agent" works the same session over the JSON socket, and each sees the other's changes.
//
//   cargo build --release && node scripts/gui-smoke.mjs [path/to/icarus] [chromium]
//
// Needs Node 22+ and Chromium. Writes screenshots to target/gui-stock.png (before the agent
// rearranges things) and target/gui-smoke.png (after).

import { spawn } from "node:child_process";
import net from "node:net";
import fs from "node:fs";
import path from "node:path";

const bin = process.argv[2] ?? "target/release/icarus";
const chromium = process.argv[3] ?? "chromium";
const GUI = "127.0.0.1:7891";
const SOCKET = "127.0.0.1:7892";
const CDP_PORT = 9339;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
};

async function until(fn, ms = 4000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn().catch(() => null);
    if (v) return v;
    if (Date.now() > end) return null;
    await sleep(50);
  }
}

// --- the session -----------------------------------------------------------------------------
const server = spawn(bin, ["gui", "--no-live", "--no-open", "--addr", GUI, "--socket", SOCKET], { stdio: ["ignore", "ignore", "pipe"] });
server.stderr.on("data", (d) => process.env.VERBOSE && process.stderr.write(d));
await sleep(500);

// --- an agent on the JSON socket ------------------------------------------------------------
const agent = net.connect(Number(SOCKET.split(":")[1]), "127.0.0.1");
let buffer = "";
let nextId = 0;
const waiting = new Map();
agent.on("data", (d) => {
  buffer += d;
  let i;
  while ((i = buffer.indexOf("\n")) >= 0) {
    const msg = JSON.parse(buffer.slice(0, i));
    buffer = buffer.slice(i + 1);
    if (msg.id !== undefined && waiting.has(msg.id)) {
      waiting.get(msg.id)(msg);
      waiting.delete(msg.id);
    }
  }
});
const ask = (request) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    waiting.set(id, (m) => (m.ok ? resolve(m.result) : reject(new Error(m.error))));
    agent.write(JSON.stringify({ id, client: "agent", request }) + "\n");
  });

// --- a browser driven over the DevTools protocol --------------------------------------------
const profile = fs.mkdtempSync(path.join("target", "gui-smoke-profile-"));
const browser = spawn(chromium, [
  "--headless=new",
  `--remote-debugging-port=${CDP_PORT}`,
  `--user-data-dir=${profile}`,
  "--window-size=1500,950",
  "--no-first-run",
  "about:blank",
]);
const targets = await until(async () => (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`)).json(), 8000);
const page = targets.find((t) => t.type === "page");
const cdp = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (cdp.onopen = r));
let cdpId = 0;
const cdpWaiting = new Map();
const errors = [];
cdp.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (msg.id && cdpWaiting.has(msg.id)) {
    cdpWaiting.get(msg.id)(msg);
    cdpWaiting.delete(msg.id);
  }
  if (msg.method === "Runtime.exceptionThrown") errors.push(msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text);
  if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") errors.push(msg.params.args.map((a) => a.value ?? a.description).join(" "));
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++cdpId;
    cdpWaiting.set(id, resolve);
    cdp.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description);
  return r.result?.result?.value;
};
const mouse = (type, x, y) => send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1 });

await send("Runtime.enable");
await send("Page.enable");
await send("Page.navigate", { url: `http://${GUI}` });

try {
  check("GUI loads and builds its layout from /ui", !!(await until(() => evaluate(`!!document.querySelector('.transport') && document.querySelectorAll('.area').length >= 5`))));

  // A person adds a track with the browser button.
  await evaluate(`[...document.querySelectorAll('button')].find(b => b.textContent === '+ Synth track').click()`);
  const tracks = await until(async () => {
    const t = await ask({ type: "get", path: "/ui/tracks" });
    return t.length === 1 ? t : null;
  });
  check("GUI button adds a track (graph + /ui/tracks in one transaction)", !!tracks, tracks ? `${tracks[0].devices} -> ${tracks[0].mix}` : "");
  check("devices panel shows the new synth's knobs", !!(await until(() => evaluate(`document.querySelectorAll('.device .knob').length >= 3`))));

  // The agent grabs a knob: the person's GUI shows whose hand is on it, then the new value.
  const osc = tracks[0].devices[0];
  await ask({ type: "gesture", gesture: { phase: "begin", node: osc, param: "freq" } });
  check("agent's touch shows on the GUI knob", !!(await until(() => evaluate(`document.querySelector('[data-param="${osc}/freq"]')?.classList.contains('held')`))));
  await ask({ type: "gesture", gesture: { phase: "move", node: osc, param: "freq", value: 440 } });
  await ask({ type: "gesture", gesture: { phase: "end", node: osc, param: "freq" } });
  check("agent's move updates the GUI knob in place", !!(await until(() => evaluate(`document.querySelector('[data-param="${osc}/freq"] .knob-value')?.textContent === '440 Hz'`))));

  // The person drags the mixer fader; the agent sees it attributed to the GUI.
  await evaluate(`[...document.querySelectorAll('.tab')].find(t => t.textContent === 'Mixer').click()`);
  const box = await until(() => evaluate(`(() => { const r = document.querySelector('.fader .fader-slot')?.getBoundingClientRect(); return r && r.height > 0 ? {x: r.x + r.width / 2, y: r.y + r.height / 2} : null })()`));
  const before = await ask({ type: "get", path: `/nodes/${tracks[0].mix}/params/gain` });
  await mouse("mousePressed", box.x, box.y);
  for (let i = 1; i <= 10; i++) {
    await mouse("mouseMoved", box.x, box.y - i * 4);
    await sleep(20);
  }
  await mouse("mouseReleased", box.x, box.y - 40);
  const after = await until(async () => {
    const v = await ask({ type: "get", path: `/nodes/${tracks[0].mix}/params/gain` });
    return v > before ? v : null;
  });
  check("dragging a GUI fader moves the param through a gesture", !!after, `${before} dB -> ${after?.toFixed?.(2)} dB`);
  const history = await ask({ type: "history" });
  check("the drag is one undo step attributed to the GUI", history.undo[0].client === "gui:human" && history.undo[0].label === `gesture ${tracks[0].mix}.gain`, `${history.undo[0].client}: ${history.undo[0].label}`);

  // The stock layout, before the agent rearranges it.
  await sleep(300);
  const stock = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync("target/gui-stock.png", Buffer.from(stock.result.data, "base64"));

  // The agent restyles and rearranges the interface by writing /ui.
  await ask({ type: "set", path: "/ui/theme/--accent", value: "#ff2d95" });
  check("agent restyles the GUI via /ui/theme", !!(await until(() => evaluate(`getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() === '#ff2d95'`))));
  await ask({ type: "set", path: "/ui/panels/log/area", value: "right" });
  check("agent moves a panel to another area via /ui/panels", !!(await until(() => evaluate(`[...document.querySelectorAll('.area')].some(a => a.style.gridArea.startsWith('right') && [...a.querySelectorAll('.tab')].some(t => t.textContent === 'Activity'))`))));

  // The agent writes a brand-new panel; it runs sandboxed and acts through the protocol.
  await ask({
    type: "transaction",
    label: "agent adds a panel",
    commands: [
      { op: "set_ui", path: "/components/hello", value: { source: "root.textContent = 'hello from a component'; icarus.set('/ui/panels/hello/title', 'Hello (self-titled)');" } },
      { op: "set_ui", path: "/panels/hello", value: { type: "hello", area: "bottom", title: "Hello", order: -1 } },
    ],
  });
  check("agent-written component panel mounts in a sandboxed iframe", !!(await until(() => evaluate(`document.querySelector('iframe.component')?.getAttribute('sandbox') === 'allow-scripts'`))));
  const title = await until(async () => {
    const t = await ask({ type: "get", path: "/ui/panels/hello/title" });
    return t === "Hello (self-titled)" ? t : null;
  });
  check("the component reached the session through the protocol", !!title);
  const hist = await ask({ type: "history" });
  check("the component's request is attributed to its panel", hist.undo[0].client === "gui:human/hello", hist.undo[0].client);

  // Undo from the GUI keyboard reverts the component's own edit.
  await send("Input.dispatchKeyEvent", { type: "keyDown", key: "z", code: "KeyZ", modifiers: 2, windowsVirtualKeyCode: 90 });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: "z", code: "KeyZ", modifiers: 2, windowsVirtualKeyCode: 90 });
  check("Ctrl+Z in the GUI undoes (key bindings from /ui/keys)", !!(await until(async () => (await ask({ type: "get", path: "/ui/panels/hello/title" })) === "Hello")));

  await sleep(300);
  const shot = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync("target/gui-smoke.png", Buffer.from(shot.result.data, "base64"));
  check("no JavaScript errors in the page", errors.length === 0, errors.join(" | "));
} catch (e) {
  check("smoke test ran to completion", false, e.stack);
} finally {
  cdp.close();
  const exited = new Promise((r) => browser.once("exit", r));
  browser.kill();
  await exited;
  server.kill();
  agent.destroy();
  fs.rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed${failed ? "" : "  · screenshots: target/gui-stock.png, target/gui-smoke.png"}`);
process.exit(failed ? 1 : 0);
