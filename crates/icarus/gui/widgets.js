// Controls that move parameters the way a hand does: press to touch (gesture begin), drag to
// move, release to let go. Every drag is one undo step, and other clients see the touch live.

import { fromNorm, toNorm } from "./client.js";

export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "class") el.className = v;
    else if (k === "style") {
      // Custom properties (--track, --c, ...) are ignored by plain assignment.
      for (const [prop, val] of Object.entries(v)) {
        if (prop.startsWith("--")) el.style.setProperty(prop, val);
        else el.style[prop] = val;
      }
    }
    else if (v !== undefined && v !== null && v !== false) el.setAttribute(k, v);
  }
  for (const c of children.flat()) if (c !== null && c !== undefined && c !== false) el.append(c);
  return el;
}

export function formatValue(spec, v) {
  if (v === undefined || v === null) return "—";
  if (spec.unit === "Hz") return v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} kHz` : `${v.toFixed(0)} Hz`;
  if (spec.unit === "dB") return `${v.toFixed(1)} dB`;
  if (spec.curve === "stepped") return String(Math.round(v));
  return v.toFixed(2);
}

/// A stable colour per client name, so the same agent always looks the same.
export function clientColor(name) {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360} 65% 62%)`;
}

const ARC = 270;

function arcPath(n) {
  const r = 14;
  const start = (-ARC / 2 - 90) * (Math.PI / 180);
  const end = start + ARC * n * (Math.PI / 180);
  const p = (a) => `${18 + r * Math.cos(a)} ${18 + r * Math.sin(a)}`;
  return `M ${p(start)} A ${r} ${r} 0 ${ARC * n > 180 ? 1 : 0} 1 ${p(end)}`;
}

/// Wires press/drag/release on `el` to a gesture on node.param.
function attachGesture(el, ctx, node, param, { vertical = true, pixels = 180 } = {}) {
  const client = ctx.client;
  let start = null;
  let latest = null;
  let frame = 0;
  const sendMove = () => {
    frame = 0;
    if (latest === null) return;
    client.gesture({ phase: "move", node, param, normalized: latest }).catch(() => {});
    latest = null;
  };
  el.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const spec = client.spec(node, param);
    if (!spec) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    start = { x: e.clientX, y: e.clientY, n: toNorm(spec, client.value(node, param)), spec };
    el.classList.add("dragging");
    client.gesture({ phase: "begin", node, param }).catch((err) => {
      ctx.toast(err.message);
      start = null;
      el.classList.remove("dragging");
    });
  });
  el.addEventListener("pointermove", (e) => {
    if (!start) return;
    const d = vertical ? start.y - e.clientY : e.clientX - start.x;
    const n = Math.min(1, Math.max(0, start.n + d / (pixels * (e.shiftKey ? 6 : 1))));
    // Optimistic: show the move now; the session's event confirms it a moment later.
    client.state.project.nodes[node].params[param] = fromNorm(start.spec, n);
    el.refresh();
    latest = n;
    if (!frame) frame = requestAnimationFrame(sendMove);
  });
  const release = () => {
    if (!start) return;
    start = null;
    el.classList.remove("dragging");
    if (frame) cancelAnimationFrame(frame);
    sendMove();
    client.gesture({ phase: "end", node, param }).catch(() => {});
  };
  el.addEventListener("pointerup", release);
  el.addEventListener("pointercancel", release);
  el.addEventListener("dblclick", () => {
    const spec = client.spec(node, param);
    client.tx([{ op: "set_param", node, param, value: spec.default }], `reset ${node}.${param}`).catch((e) => ctx.toast(e.message));
  });
}

/// Marks a control held by someone else with their name and colour.
function showHolder(el, ctx, node, param) {
  const holder = ctx.client.heldBy(node, param);
  const other = holder && holder !== ctx.client.name;
  el.classList.toggle("held", !!other);
  el.style.setProperty("--holder", other ? clientColor(holder) : "transparent");
  el.title = other ? `${holder} is holding this` : `${node}.${param} — drag to change, shift for fine, double-click to reset`;
}

export function knob(ctx, node, param, { label } = {}) {
  const spec = ctx.client.spec(node, param);
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 36 36");
  svg.innerHTML = `<path class="track" d="${arcPath(1)}"/><path class="fill"/><circle class="cap" cx="18" cy="18" r="9"/>`;
  const valueEl = h("div", { class: "knob-value" });
  const el = h("div", { class: "knob", "data-param": `${node}/${param}` }, h("div", { class: "knob-label" }, label ?? spec?.name ?? param), svg, valueEl);
  el.refresh = () => {
    const s = ctx.client.spec(node, param);
    if (!s) return;
    const v = ctx.client.value(node, param);
    const n = toNorm(s, v);
    svg.querySelector(".fill").setAttribute("d", n > 0.002 ? arcPath(n) : "");
    valueEl.textContent = formatValue(s, v);
    el.classList.toggle("automated", !!ctx.client.state.project.nodes[node]?.automation?.[param]?.length);
    showHolder(el, ctx, node, param);
  };
  attachGesture(el, ctx, node, param);
  el.refresh();
  return el;
}

export function fader(ctx, node, param) {
  const fill = h("div", { class: "fader-fill" });
  const thumb = h("div", { class: "fader-thumb" });
  const valueEl = h("div", { class: "fader-value" });
  const el = h("div", { class: "fader", "data-param": `${node}/${param}` }, h("div", { class: "fader-slot" }, fill, thumb), valueEl);
  el.refresh = () => {
    const s = ctx.client.spec(node, param);
    if (!s) return;
    const v = ctx.client.value(node, param);
    const n = toNorm(s, v);
    fill.style.height = `${n * 100}%`;
    thumb.style.bottom = `${n * 100}%`;
    valueEl.textContent = formatValue(s, v);
    showHolder(el, ctx, node, param);
  };
  attachGesture(el, ctx, node, param, { pixels: 140 });
  el.refresh();
  return el;
}
