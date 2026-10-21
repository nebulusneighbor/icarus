// Automation: draw a parameter's lane over the loop. Click to add a point, drag to move it,
// right-click to delete. Each edit writes the whole lane at /nodes/<id>/automation/<param>.

import { fromNorm, toNorm } from "../client.js";
import { formatValue, h } from "../widgets.js";

export default function automation(el, ctx, panel, id) {
  const { client } = ctx;
  const project = client.state.project;
  const state = (ctx.panelState[id] ??= {});

  const options = [];
  for (const [node, n] of Object.entries(project.nodes)) {
    for (const spec of client.state.devices[n.kind]?.params ?? []) {
      options.push({ key: `${node}.${spec.id}`, node, param: spec.id, spec, automated: !!n.automation?.[spec.id]?.length });
    }
  }
  if (!options.find((o) => o.key === state.lane)) {
    state.lane =
      options.find((o) => o.automated)?.key ??
      options.find((o) => o.node === ctx.selection.node)?.key ??
      options[0]?.key;
  }
  const lane = options.find((o) => o.key === state.lane);

  const select = h(
    "select",
    { onchange: (e) => ((state.lane = e.target.value), ctx.render()) },
    options.map((o) => h("option", { value: o.key, selected: o.key === state.lane }, `${o.automated ? "● " : ""}${o.key}`)),
  );
  const readout = h("span", { class: "muted" });
  const canvas = h("canvas", { class: "lane" });
  el.append(
    h(
      "div",
      { class: "toolbar" },
      select,
      readout,
      h("span", { class: "spacer" }),
      h("span", { class: "muted" }, "click: add · drag: move · right-click: delete"),
      lane
        ? h("button", { onclick: () => client.set(`/nodes/${lane.node}/automation/${lane.param}`, null).catch((e) => ctx.toast(e.message)) }, "Clear lane")
        : null,
    ),
    h("div", { class: "lane-wrap" }, canvas),
  );
  if (!lane) return {};

  let points = (project.nodes[lane.node].automation?.[lane.param] ?? []).map((p) => ({ ...p }));
  const length = () => client.state.project.length_s;
  const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  function geometry() {
    const r = canvas.getBoundingClientRect();
    return { w: r.width, h: r.height, pad: 10 };
  }
  const toX = (t, g) => g.pad + (t / length()) * (g.w - 2 * g.pad);
  const toY = (v, g) => g.pad + (1 - toNorm(lane.spec, v)) * (g.h - 2 * g.pad);
  const fromXY = (x, y, g) => ({
    time_s: Math.min(length(), Math.max(0, ((x - g.pad) / (g.w - 2 * g.pad)) * length())),
    value: fromNorm(lane.spec, 1 - (y - g.pad) / (g.h - 2 * g.pad)),
  });

  function draw() {
    const g = geometry();
    const dpr = devicePixelRatio || 1;
    canvas.width = g.w * dpr;
    canvas.height = g.h * dpr;
    const c = canvas.getContext("2d");
    c.scale(dpr, dpr);
    c.clearRect(0, 0, g.w, g.h);

    // Beat grid.
    const beat = 60 / project.tempo_bpm;
    for (let t = 0, i = 0; t <= length(); t += beat, i++) {
      c.strokeStyle = i % 4 === 0 ? css("--panel-3") : css("--panel-2");
      c.beginPath();
      c.moveTo(toX(t, g), 0);
      c.lineTo(toX(t, g), g.h);
      c.stroke();
    }

    c.strokeStyle = css("--accent");
    c.lineWidth = 2;
    c.beginPath();
    if (points.length) {
      c.moveTo(toX(0, g), toY(points[0].value, g));
      for (const p of points) c.lineTo(toX(p.time_s, g), toY(p.value, g));
      c.lineTo(toX(length(), g), toY(points.at(-1).value, g));
    } else {
      c.setLineDash([4, 4]);
      const v = client.value(lane.node, lane.param);
      c.moveTo(toX(0, g), toY(v, g));
      c.lineTo(toX(length(), g), toY(v, g));
    }
    c.stroke();
    c.setLineDash([]);
    c.fillStyle = css("--accent");
    for (const p of points) {
      c.beginPath();
      c.arc(toX(p.time_s, g), toY(p.value, g), 4, 0, Math.PI * 2);
      c.fill();
    }

    const pos = client.state.transport.position_s ?? 0;
    if (client.state.transport.playing) {
      c.strokeStyle = css("--accent-2");
      c.lineWidth = 1;
      c.beginPath();
      c.moveTo(toX(pos % length(), g), 0);
      c.lineTo(toX(pos % length(), g), g.h);
      c.stroke();
    }
    readout.textContent = points.length ? `${points.length} points` : `not automated · ${formatValue(lane.spec, client.value(lane.node, lane.param))}`;
  }

  const save = () =>
    client
      .set(
        `/nodes/${lane.node}/automation/${lane.param}`,
        [...points].sort((a, b) => a.time_s - b.time_s),
      )
      .catch((e) => ctx.toast(e.message));

  const hit = (x, y, g) => points.findIndex((p) => Math.hypot(toX(p.time_s, g) - x, toY(p.value, g) - y) < 8);

  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const g = geometry();
    const r = canvas.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    let i = hit(x, y, g);
    if (i < 0) {
      points.push(fromXY(x, y, g));
      i = points.length - 1;
    }
    canvas.setPointerCapture(e.pointerId);
    canvas.onpointermove = (m) => {
      points[i] = fromXY(m.clientX - r.left, m.clientY - r.top, g);
      draw();
    };
    canvas.onpointerup = () => {
      canvas.onpointermove = canvas.onpointerup = null;
      save();
    };
    draw();
  });
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const r = canvas.getBoundingClientRect();
    const i = hit(e.clientX - r.left, e.clientY - r.top, geometry());
    if (i >= 0) {
      points.splice(i, 1);
      save();
    }
  });

  const observer = new ResizeObserver(draw);
  observer.observe(canvas);
  return {
    update: (msg) => (msg.type === "tick" || msg.type === "event") && draw(),
    destroy: () => observer.disconnect(),
  };
}
