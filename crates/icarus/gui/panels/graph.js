// The raw signal graph: every node and connection, whether or not a track claims it.
// Drag from a node's right port to another node's left port to connect; click a cable to
// disconnect; drag a node's title to move it. Positions are shared, at /ui/graph/positions.

import { clientColor, h } from "../widgets.js";
import { trackOf, tracks } from "../tracks.js";

const W = 150;
const H = 54;

function autoLayout(project) {
  const ids = Object.keys(project.nodes);
  const inputs = Object.fromEntries(ids.map((id) => [id, []]));
  for (const c of project.connections) inputs[c.to]?.push(c.from);
  const depth = {};
  const visit = (id, seen = new Set()) => {
    if (depth[id] !== undefined) return depth[id];
    if (seen.has(id)) return 0;
    seen.add(id);
    depth[id] = Math.max(-1, ...inputs[id].map((i) => visit(i, seen))) + 1;
    return depth[id];
  };
  ids.forEach((id) => visit(id));
  depth.master = Math.max(...Object.values(depth)) + (inputs.master.length ? 0 : 1);
  const rows = {};
  const pos = {};
  for (const id of ids) {
    const d = depth[id];
    rows[d] = (rows[d] ?? 0) + 1;
    pos[id] = [24 + d * (W + 60), 20 + (rows[d] - 1) * (H + 30)];
  }
  return pos;
}

export default function graph(el, ctx) {
  const { client } = ctx;
  const project = client.state.project;
  const saved = project.ui.graph?.positions ?? {};
  const auto = autoLayout(project);
  const pos = Object.fromEntries(Object.keys(project.nodes).map((id) => [id, saved[id] ?? auto[id]]));

  const canvas = h("div", { class: "graph" });
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.classList.add("cables");
  canvas.append(svg);
  el.append(
    h(
      "div",
      { class: "toolbar" },
      h("span", { class: "muted" }, "Drag ● → ● to connect · click a cable to disconnect · Delete removes the selected node"),
      h("span", { class: "spacer" }),
      h("button", { onclick: () => client.set("/ui/graph/positions", {}).catch((e) => ctx.toast(e.message)) }, "Auto layout"),
    ),
    canvas,
  );

  const portOut = (id) => [pos[id][0] + W, pos[id][1] + H / 2];
  const portIn = (id) => [pos[id][0], pos[id][1] + H / 2];
  const curve = ([x1, y1], [x2, y2]) => {
    const dx = Math.max(40, Math.abs(x2 - x1) / 2);
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  };

  function drawCables() {
    svg.replaceChildren();
    let maxX = 0;
    let maxY = 0;
    for (const [x, y] of Object.values(pos)) {
      maxX = Math.max(maxX, x + W + 40);
      maxY = Math.max(maxY, y + H + 40);
    }
    svg.setAttribute("width", maxX);
    svg.setAttribute("height", maxY);
    for (const c of project.connections) {
      if (!pos[c.from] || !pos[c.to]) continue;
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", curve(portOut(c.from), portIn(c.to)));
      path.classList.add("cable");
      const t = trackOf(client, c.from);
      if (t >= 0) path.style.stroke = tracks(client)[t].color;
      path.addEventListener("click", () =>
        client.tx([{ op: "disconnect", from: c.from, to: c.to }], `disconnect ${c.from} → ${c.to}`).catch((e) => ctx.toast(e.message)),
      );
      path.append(Object.assign(document.createElementNS("http://www.w3.org/2000/svg", "title"), { textContent: `${c.from} → ${c.to} (click to disconnect)` }));
      svg.append(path);
    }
  }

  for (const [id, node] of Object.entries(project.nodes)) {
    const t = trackOf(client, id);
    const holders = Object.entries(client.state.touches)
      .filter(([k]) => k.startsWith(`${id}/`))
      .map(([, who]) => who);
    const box = h(
      "div",
      {
        class: `node${ctx.selection.node === id ? " selected" : ""}${holders.length ? " held" : ""}`,
        style: { left: `${pos[id][0]}px`, top: `${pos[id][1]}px`, "--track": t >= 0 ? tracks(client)[t].color : "var(--border)", "--holder": holders[0] ? clientColor(holders[0]) : "transparent" },
        title: holders.length ? `held by ${holders.join(", ")}` : "",
      },
      h("div", { class: "node-title" }, id),
      h("div", { class: "node-kind muted" }, node.kind, t >= 0 ? ` · ${tracks(client)[t].name}` : ""),
      id === "master" ? null : h("div", { class: "port out", "data-node": id }),
      node.kind === "osc" || node.kind === "noise" ? null : h("div", { class: "port in", "data-node": id }),
    );
    canvas.append(box);

    // Move by dragging the body; select on click.
    box.addEventListener("pointerdown", (e) => {
      if (e.target.classList.contains("port")) return;
      const start = { x: e.clientX, y: e.clientY, p: [...pos[id]] };
      let moved = false;
      box.setPointerCapture(e.pointerId);
      box.onpointermove = (m) => {
        const dx = m.clientX - start.x;
        const dy = m.clientY - start.y;
        if (Math.abs(dx) + Math.abs(dy) > 3) moved = true;
        pos[id] = [Math.max(0, start.p[0] + dx), Math.max(0, start.p[1] + dy)];
        box.style.left = `${pos[id][0]}px`;
        box.style.top = `${pos[id][1]}px`;
        drawCables();
      };
      box.onpointerup = () => {
        box.onpointermove = box.onpointerup = null;
        if (moved) client.set(`/ui/graph/positions/${id}`, pos[id].map(Math.round)).catch((e) => ctx.toast(e.message));
        else ctx.select(t >= 0 ? { track: t, node: id } : { node: id });
      };
    });
  }

  // Patch cables: drag from an output port, drop on an input port.
  canvas.addEventListener("pointerdown", (e) => {
    if (!e.target.classList.contains("out")) return;
    e.stopPropagation();
    const from = e.target.dataset.node;
    const rect = canvas.getBoundingClientRect();
    const temp = document.createElementNS("http://www.w3.org/2000/svg", "path");
    temp.classList.add("cable", "pending");
    svg.append(temp);
    const move = (m) => temp.setAttribute("d", curve(portOut(from), [m.clientX - rect.left + canvas.scrollLeft, m.clientY - rect.top + canvas.scrollTop]));
    const up = (u) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      temp.remove();
      const target = document.elementFromPoint(u.clientX, u.clientY);
      const to = target?.classList.contains("in") ? target.dataset.node : target?.closest(".node")?.querySelector(".in")?.dataset.node;
      if (to && to !== from) client.tx([{ op: "connect", from, to }], `connect ${from} → ${to}`).catch((err) => ctx.toast(err.message));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });

  drawCables();
}
