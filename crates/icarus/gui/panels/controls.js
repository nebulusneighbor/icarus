// Declarative panels: compose a panel from widgets by writing data, no code needed. The native
// app renders the same `controls` panels, so a panel an agent builds works in both GUIs.
//
//   {"type": "controls", "props": {"widgets": [
//     {"knob": "t1_osc.freq", "label": "Pitch"}, {"fader": "t1_mix.gain"},
//     {"xy": ["t1_filter.cutoff", "t1_filter.resonance"]}, {"label": "Section B"},
//     {"button": "Drop", "request": {"type": "set", "path": "/nodes/t1_filter/params/cutoff", "value": 200}},
//     {"row": [...]}, {"column": [...]}]}}

import { fromNorm, toNorm } from "../client.js";
import { fader, h, knob } from "../widgets.js";

const split = (target) => (target ?? "").split(".");

function xy(ctx, [x, y] = [], size = 180) {
  const { client } = ctx;
  const axes = [split(x), split(y)].map(([node, param]) => ({ node, param }));
  const dot = h("div", { class: "xy-dot" });
  const pad = h("div", { class: "xy-pad", style: { width: `${size}px`, height: `${size}px` } }, dot, h("div", { class: "xy-label muted" }, `x ${x ?? "—"}  y ${y ?? "—"}`));
  const live = axes.filter((a) => client.spec(a.node, a.param));
  const draw = () => {
    const n = axes.map((a) => (client.spec(a.node, a.param) ? toNorm(client.spec(a.node, a.param), client.value(a.node, a.param)) : 0.5));
    dot.style.left = `${n[0] * 100}%`;
    dot.style.top = `${(1 - n[1]) * 100}%`;
  };
  let dragging = false;
  const move = (e) => {
    const r = pad.getBoundingClientRect();
    const n = [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height))];
    axes.forEach((a, i) => {
      const spec = client.spec(a.node, a.param);
      if (!spec) return;
      client.state.project.nodes[a.node].params[a.param] = fromNorm(spec, n[i]);
      client.gesture({ phase: "move", node: a.node, param: a.param, normalized: n[i] }).catch(() => {});
    });
    draw();
  };
  pad.addEventListener("pointerdown", (e) => {
    pad.setPointerCapture(e.pointerId);
    dragging = true;
    for (const a of live) client.gesture({ phase: "begin", node: a.node, param: a.param }).catch((err) => ctx.toast(err.message));
    move(e);
  });
  pad.addEventListener("pointermove", (e) => dragging && move(e));
  pad.addEventListener("pointerup", () => {
    dragging = false;
    for (const a of live) client.gesture({ phase: "end", node: a.node, param: a.param }).catch(() => {});
  });
  // Refresh with the other controls when either axis changes.
  for (const a of live) {
    const marker = h("span", { "data-param": `${a.node}/${a.param}`, style: { display: "none" } });
    marker.refresh = draw;
    pad.append(marker);
  }
  draw();
  return pad;
}

function widget(ctx, w) {
  const missing = (t) => h("div", { class: "error" }, `no ${t}`);
  if (w.knob) {
    const [node, param] = split(w.knob);
    return ctx.client.spec(node, param) ? knob(ctx, node, param, { label: w.label }) : missing(w.knob);
  }
  if (w.fader) {
    const [node, param] = split(w.fader);
    if (!ctx.client.spec(node, param)) return missing(w.fader);
    const el = fader(ctx, node, param);
    el.style.height = `${w.height ?? 160}px`;
    el.style.flex = "none";
    return el;
  }
  if (w.xy) return xy(ctx, w.xy, w.size);
  if (w.label) return h("strong", { class: "controls-label" }, w.label);
  if (w.button) {
    return h("button", { onclick: () => ctx.client.request(w.request).catch((e) => ctx.toast(`${w.button}: ${e.message}`)) }, w.button);
  }
  if (w.row) return h("div", { class: "controls-row" }, w.row.map((i) => widget(ctx, i)));
  if (w.column) return h("div", { class: "controls-column" }, w.column.map((i) => widget(ctx, i)));
  return h("div", { class: "error" }, `unknown widget ${JSON.stringify(w)}`);
}

export default function controls(el, ctx, panel, id) {
  const widgets = panel.props?.widgets ?? [];
  if (!widgets.length) {
    el.append(
      h(
        "div",
        { class: "pad muted" },
        h("p", {}, "An empty controls panel. Fill props.widgets with knobs, faders, XY pads, labels and buttons:"),
        h("code", {}, '[{"knob": "node.param"}, {"fader": "node.param"}, {"xy": ["a.p", "b.q"]}, {"button": "Go", "request": {...}}]'),
        h("p", {}, h("button", { onclick: () => ctx.editJson(`/ui/panels/${id}`) }, "Edit this panel")),
      ),
    );
    return;
  }
  el.append(h("div", { class: "controls" }, widgets.map((w) => widget(ctx, w))));
}
