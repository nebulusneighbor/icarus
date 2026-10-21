// Inspector: the whole session as its path tree, the same tree agents, OSC and VR see.
// Expand anything; edit any value (as JSON) and press Enter to write it.

import { h } from "../widgets.js";

export default function inspector(el, ctx, panel, id) {
  const { client } = ctx;
  const state = (ctx.panelState[id] ??= { open: new Set(["/"]), path: "" });
  const tree = h("div", { class: "tree" });
  const go = h("input", {
    placeholder: "/nodes/…  (Enter to open)",
    value: state.path,
    onkeydown: (e) => {
      if (e.key !== "Enter") return;
      state.path = e.target.value.trim();
      if (state.path) ctx.editJson(state.path);
    },
  });
  el.append(h("div", { class: "toolbar" }, go), tree);

  const leaves = new Map();

  const isScalar = (v) => v === null || typeof v !== "object";

  async function row(path, depth, container) {
    const name = path === "/" ? "/" : path.split("/").at(-1);
    const line = h("div", { class: "tree-row", style: { paddingLeft: `${6 + depth * 12}px` } });
    container.append(line);
    let children = null;
    try {
      children = await client.request({ type: "list", path });
    } catch {}

    if (children) {
      const open = state.open.has(path);
      const kids = h("div");
      const twisty = h("span", { class: "twisty" }, open ? "▾" : "▸");
      twisty.onclick = () => {
        open ? state.open.delete(path) : state.open.add(path);
        ctx.render();
      };
      line.append(twisty, h("span", { class: "key", onclick: twisty.onclick }, name), h("span", { class: "muted" }, ` ${children.length}`));
      // Some branches also hold a value, e.g. a param has a value and normalized/spec children.
      if (path !== "/") {
        const value = await client.get(path).catch(() => undefined);
        if (value !== undefined && isScalar(value)) editable(path, value, line);
      }
      container.append(kids);
      if (open) for (const child of children) await row(child, depth + 1, kids);
      return;
    }

    let value;
    try {
      value = await client.get(path);
    } catch (e) {
      value = `⚠ ${e.message}`;
    }
    line.append(h("span", { class: "twisty" }, ""), h("span", { class: "key" }, name));
    editable(path, value, line);
  }

  function editable(path, value, line) {
    const input = h("input", { class: "value", spellcheck: "false" });
    const show = (v) => {
      input.value = typeof v === "string" && v.includes("\n") ? "(multi-line, click ⤢)" : JSON.stringify(v);
    };
    show(value);
    input.onkeydown = async (e) => {
      if (e.key !== "Enter") return;
      let next;
      try {
        next = JSON.parse(input.value);
      } catch {
        next = input.value;
      }
      try {
        await client.set(path, next);
        input.classList.remove("bad");
      } catch (err) {
        input.classList.add("bad");
        ctx.toast(err.message);
      }
    };
    line.append(input, h("button", { class: "icon", title: "Open in editor", onclick: () => ctx.editJson(path) }, "⤢"));
    leaves.set(path, show);
  }

  row("/", 0, tree);

  return {
    // Live values: update leaves in place as changes arrive from anyone.
    update(msg) {
      if (msg.type !== "event") return;
      for (const c of msg.event.changes) {
        leaves.get(c.path)?.(c.value);
        if (c.normalized !== undefined) leaves.get(`${c.path}/normalized`)?.(c.normalized);
        // A row showing a whole object (e.g. /touches) changes when anything under it does.
        for (const [path, show] of leaves) {
          if (c.path.startsWith(`${path}/`)) client.get(path).then(show, () => {});
        }
      }
    },
  };
}
