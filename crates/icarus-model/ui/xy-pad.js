// XY pad: a custom panel. It runs in a sandboxed frame and can only use the protocol, through
// `icarus` (request, get, set, gesture, on, props, panel, state). Anyone can replace this
// source at /ui/components/xy-pad/source, or write a new component next to it.

const params = [];
for (const [id, node] of Object.entries(icarus.state.project.nodes)) {
  for (const spec of icarus.state.devices[node.kind]?.params ?? []) params.push(`${id}.${spec.id}`);
}

root.innerHTML = `
  <div class="bar">
    X <select id="x"></select>
    Y <select id="y"></select>
  </div>
  <div id="pad"><div id="dot"></div></div>`;
const style = document.createElement("style");
style.textContent = `
  .bar { display: flex; gap: 6px; align-items: center; padding: 6px; color: var(--muted); }
  select { background: var(--panel-2); color: var(--text); border: 1px solid var(--border); border-radius: var(--radius); }
  #pad { position: absolute; inset: 36px 8px 8px 8px; background: var(--panel-2); border: 1px solid var(--border);
         border-radius: var(--radius); touch-action: none; cursor: crosshair;
         background-image: linear-gradient(var(--border) 1px, transparent 1px), linear-gradient(90deg, var(--border) 1px, transparent 1px);
         background-size: 25% 25%; }
  #dot { position: absolute; width: 16px; height: 16px; margin: -8px; border-radius: 50%;
         background: var(--accent); box-shadow: 0 0 12px var(--accent); }`;
document.head.append(style);

const axes = { x: icarus.props.x || "", y: icarus.props.y || "" };
const pos = { x: 0.5, y: 0.5 };
for (const axis of ["x", "y"]) {
  const select = document.getElementById(axis);
  select.innerHTML = `<option value="">—</option>` + params.map((p) => `<option>${p}</option>`).join("");
  select.value = axes[axis];
  // The pad configures itself by writing its own props into the shared UI document.
  select.onchange = () => icarus.set(`/ui/panels/${icarus.panel}/props/${axis}`, select.value);
}

const dot = document.getElementById("dot");
const pad = document.getElementById("pad");
const draw = () => {
  dot.style.left = `${pos.x * 100}%`;
  dot.style.top = `${(1 - pos.y) * 100}%`;
};
const split = (p) => p.split(".");

async function load() {
  for (const axis of ["x", "y"]) {
    if (!axes[axis]) continue;
    const [node, param] = split(axes[axis]);
    pos[axis] = await icarus.get(`/nodes/${node}/params/${param}/normalized`);
  }
  draw();
}

icarus.on((event) => {
  for (const change of event.changes) {
    for (const axis of ["x", "y"]) {
      const [node, param] = split(axes[axis] || ".");
      if (change.path === `/nodes/${node}/params/${param}` && change.normalized != null) pos[axis] = change.normalized;
    }
  }
  draw();
});

let dragging = false;
pad.onpointerdown = async (e) => {
  pad.setPointerCapture(e.pointerId);
  dragging = true;
  for (const axis of ["x", "y"]) {
    if (!axes[axis]) continue;
    const [node, param] = split(axes[axis]);
    await icarus.gesture({ phase: "begin", node, param }).catch(() => {});
  }
  move(e);
};
pad.onpointermove = (e) => dragging && move(e);
pad.onpointerup = () => {
  dragging = false;
  for (const axis of ["x", "y"]) {
    if (!axes[axis]) continue;
    const [node, param] = split(axes[axis]);
    icarus.gesture({ phase: "end", node, param }).catch(() => {});
  }
};

function move(e) {
  const r = pad.getBoundingClientRect();
  pos.x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  pos.y = Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height));
  draw();
  for (const axis of ["x", "y"]) {
    if (!axes[axis]) continue;
    const [node, param] = split(axes[axis]);
    icarus.gesture({ phase: "move", node, param, normalized: pos[axis] }).catch(() => {});
  }
}

load();
