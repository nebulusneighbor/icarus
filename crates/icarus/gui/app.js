// The stock GUI shell. It builds itself from the /ui document in the session: the layout
// grid, which panels sit where, the theme, key bindings and custom panel code. Change /ui
// from anywhere (this GUI's edit mode, an agent, a script) and every open GUI rebuilds.

import { Client } from "./client.js";
import { h } from "./widgets.js";
import { mountComponent, TEMPLATE } from "./sandbox.js";
import { tracks } from "./tracks.js";
import transport from "./panels/transport.js";
import browser from "./panels/browser.js";
import mixer from "./panels/mixer.js";
import devices from "./panels/devices.js";
import graph from "./panels/graph.js";
import automation from "./panels/automation.js";
import inspector from "./panels/inspector.js";
import log from "./panels/log.js";
import controls from "./panels/controls.js";

/// Built-in panel types. A component in /ui/components with the same name replaces one.
export const BUILTINS = { transport, browser, mixer, devices, graph, automation, inspector, log, controls };

const store = {
  get(k, fallback) {
    try {
      return localStorage.getItem(k) ?? fallback;
    } catch {
      return fallback;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem(k, v);
    } catch {}
  },
};

const client = new Client(`ws://${location.host}`, `gui:${store.get("icarus.name", "human")}`);

/// Shared with every panel.
const ctx = {
  client,
  editing: false,
  selection: { track: null, node: null },
  /// Per-panel UI state that should survive rebuilds (scroll, expanded rows, chosen lane).
  panelState: {},
  /// Recent change events, newest last, for the activity panel and presence.
  log: [],
  select(sel) {
    ctx.selection = { track: null, node: null, ...sel };
    scheduleRender();
  },
  toast,
  editJson,
  rename(name) {
    store.set("icarus.name", name);
    client.name = `gui:${name}`;
  },
  render: () => scheduleRender(),
  actions: {},
};

ctx.actions = {
  "transport.toggle": () => client.set("/transport/playing", !client.state.transport.playing).catch((e) => toast(e.message)),
  undo: () => client.request({ type: "undo" }).catch((e) => toast(e.message)),
  redo: () => client.request({ type: "redo" }).catch((e) => toast(e.message)),
  "ui.edit": () => {
    ctx.editing = !ctx.editing;
    scheduleRender();
  },
  "ui.addPanel": addPanel,
  "ui.newComponent": newComponent,
  "selection.delete": () => {
    const id = ctx.selection.node;
    if (id && id !== "master") client.tx([{ op: "remove_node", id }], `remove ${id}`).catch((e) => toast(e.message));
  },
};

let mounted = [];
let renderQueued = false;

function scheduleRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    render();
  });
}

function render() {
  const ui = client.state.project.ui;
  for (const m of mounted) m.instance?.destroy?.();
  mounted = [];

  for (const [k, v] of Object.entries(ui.theme ?? {})) document.documentElement.style.setProperty(k, v);
  document.body.classList.toggle("editing", ctx.editing);
  if (ctx.selection.track !== null && !tracks(client)[ctx.selection.track]) ctx.selection.track = null;
  if (ctx.selection.node && !client.state.project.nodes[ctx.selection.node]) ctx.selection.node = null;

  const layout = ui.layout ?? {};
  const areas = layout.areas ?? [["main"]];
  const app = document.getElementById("app");
  app.replaceChildren();
  Object.assign(app.style, {
    gridTemplateColumns: layout.columns ?? "1fr",
    gridTemplateRows: layout.rows ?? "1fr",
    gridTemplateAreas: areas.map((row) => `"${row.join(" ")}"`).join(" "),
  });

  const byArea = {};
  for (const [id, panel] of Object.entries(ui.panels ?? {})) (byArea[panel.area] ??= []).push([id, panel]);
  const areaNames = [...new Set(areas.flat())];
  for (const area of areaNames) {
    const panels = (byArea[area] ?? []).sort((a, b) => (a[1].order ?? 0) - (b[1].order ?? 0));
    app.append(renderArea(area, panels));
  }
  const orphans = Object.entries(ui.panels ?? {}).filter(([, p]) => !areaNames.includes(p.area));
  if (orphans.length) toast(`panels in unknown areas: ${orphans.map(([id]) => id).join(", ")}`);
}

function renderArea(area, panels) {
  const section = h("section", { class: "area", style: { gridArea: area } });
  const tabKey = `icarus.tab.${area}`;
  let active = store.get(tabKey, panels[0]?.[0]);
  if (!panels.some(([id]) => id === active)) active = panels[0]?.[0];

  const bare = !ctx.editing && (!panels.length || (panels.length === 1 && panels[0][1].type === "transport"));
  if (!bare) {
    const tabs = h("div", { class: "tabs" });
    for (const [id, panel] of panels) {
      const tab = h(
        "button",
        {
          class: `tab${id === active ? " active" : ""}`,
          onclick: () => {
            store.set(tabKey, id);
            scheduleRender();
          },
        },
        panel.title ?? id,
      );
      tabs.append(tab);
    }
    if (ctx.editing) {
      tabs.append(h("span", { class: "spacer" }), h("span", { class: "area-name" }, area));
      if (active) {
        tabs.append(
          h("button", { class: "icon", title: "Edit this panel's JSON", onclick: () => editJson(`/ui/panels/${active}`) }, "⚙"),
          h(
            "button",
            {
              class: "icon",
              title: "Remove this panel",
              onclick: () => client.set(`/ui/panels/${active}`, null).catch((e) => toast(e.message)),
            },
            "✕",
          ),
        );
      }
    }
    section.append(tabs);
  }

  const body = h("div", { class: "panel-body" });
  section.append(body);
  const entry = panels.find(([id]) => id === active);
  if (entry) {
    const [id, panel] = entry;
    body.dataset.type = panel.type;
    mounted.push({ id, instance: mountPanel(body, id, panel) });
  } else if (ctx.editing) {
    body.append(h("div", { class: "empty" }, `Empty area "${area}". Add a panel to it.`));
  }
  return section;
}

function mountPanel(el, id, panel) {
  const component = client.state.project.ui.components?.[panel.type];
  try {
    if (component?.source) return mountComponent(el, ctx, panel, id, component);
    const builtin = BUILTINS[panel.type];
    if (builtin) return builtin(el, ctx, panel, id) ?? {};
  } catch (err) {
    console.error(err);
    el.append(h("pre", { class: "error" }, `${panel.type}: ${err.message}`));
    return {};
  }
  el.append(h("div", { class: "empty" }, `Unknown panel type "${panel.type}". Define it at /ui/components/${panel.type}.`));
  return {};
}

/// Updates controls in place for changes that don't need a rebuild (param values, touches).
function refreshControls(event) {
  const touched = new Set();
  for (const c of event.changes) {
    const seg = c.path.split("/").filter(Boolean);
    if (seg[0] === "nodes" && seg[2] === "params") touched.add(`${seg[1]}/${seg[3]}`);
    if (seg[0] === "touches") touched.add(`${seg[1]}/${seg[2]}`);
  }
  for (const key of touched) {
    for (const el of document.querySelectorAll(`[data-param="${CSS.escape(key)}"]`)) el.refresh?.();
  }
}

client.on((msg) => {
  if (msg.type === "reload") return scheduleRender();
  if (msg.type === "event") {
    ctx.log.push({ at: new Date(), ...msg.event });
    if (ctx.log.length > 500) ctx.log.shift();
    if (msg.structural) scheduleRender();
    else refreshControls(msg.event);
  }
  for (const m of mounted) m.instance?.update?.(msg);
});

// Playhead: poll the transport while playing.
setInterval(async () => {
  if (client.status !== "open" || !client.state.transport.playing) return;
  try {
    client.state.transport = await client.get("/transport");
    for (const m of mounted) m.instance?.update?.({ type: "tick" });
  } catch {}
}, 80);

// Key bindings come from /ui/keys, e.g. {"ctrl+z": "undo"}.
window.addEventListener("keydown", (e) => {
  if (e.target.closest("input, textarea, select, [contenteditable]")) return;
  const combo = [e.ctrlKey || e.metaKey ? "ctrl" : "", e.shiftKey ? "shift" : "", e.altKey ? "alt" : "", e.key.toLowerCase()]
    .filter(Boolean)
    .join("+");
  const action = client.state.project?.ui.keys?.[combo];
  if (action && ctx.actions[action]) {
    e.preventDefault();
    ctx.actions[action]();
  }
});

function toast(message) {
  const el = h("div", { class: "toast" }, message);
  document.getElementById("toasts").append(el);
  setTimeout(() => el.remove(), 4000);
}

/// Opens a JSON editor for any path. Saving writes it back with `set`.
async function editJson(path, { title, initial } = {}) {
  let value = initial;
  if (value === undefined) {
    try {
      value = await client.get(path);
    } catch (e) {
      return toast(e.message);
    }
  }
  const isSource = typeof value === "string" && value.includes("\n");
  const text = h("textarea", { class: "editor", spellcheck: "false" });
  text.value = isSource ? value : JSON.stringify(value, null, 2);
  const close = () => overlay.remove();
  const save = async () => {
    let next;
    try {
      next = isSource ? text.value : JSON.parse(text.value);
    } catch (e) {
      return toast(`Invalid JSON: ${e.message}`);
    }
    try {
      await client.set(path, next);
      close();
    } catch (e) {
      toast(e.message);
    }
  };
  const overlay = h(
    "div",
    { class: "modal-backdrop", onclick: (e) => e.target === overlay && close() },
    h(
      "div",
      { class: "modal" },
      h("div", { class: "modal-head" }, h("strong", {}, title ?? path), h("span", { class: "spacer" }), h("button", { onclick: close }, "Cancel"), h("button", { class: "primary", onclick: save }, "Save")),
      text,
    ),
  );
  text.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") save();
    if (e.key === "Escape") close();
  });
  document.body.append(overlay);
  text.focus();
}

function addPanel() {
  const ui = client.state.project.ui;
  const types = [...Object.keys(BUILTINS), ...Object.keys(ui.components ?? {})];
  const areas = [...new Set((ui.layout?.areas ?? []).flat())];
  let n = 1;
  while (ui.panels?.[`panel${n}`]) n++;
  editJson(`/ui/panels/panel${n}`, {
    title: `New panel — type is one of: ${[...new Set(types)].join(", ")}; area is one of: ${areas.join(", ")}`,
    initial: { type: "controls", area: areas.includes("bottom") ? "bottom" : areas[0], title: `Panel ${n}`, order: 9, props: { widgets: [] } },
  });
}

function newComponent() {
  const ui = client.state.project.ui;
  let n = 1;
  while (ui.components?.[`custom${n}`]) n++;
  const name = `custom${n}`;
  const area = (ui.layout?.areas ?? [["main"]]).flat().includes("main") ? "main" : ui.layout.areas[0][0];
  client
    .tx(
      [
        { op: "set_ui", path: `/components/${name}`, value: { description: "A custom panel", source: TEMPLATE } },
        { op: "set_ui", path: `/panels/${name}`, value: { type: name, area, title: name, order: 9 } },
      ],
      `new component ${name}`,
    )
    .then(() => {
      store.set(`icarus.tab.${area}`, name);
      editJson(`/ui/components/${name}/source`, { title: `Source of ${name} (Ctrl+Enter to save)` });
    })
    .catch((e) => toast(e.message));
}

client.connect().then(scheduleRender);
