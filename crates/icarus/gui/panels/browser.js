// Browser: add tracks and devices, open custom panels, save and load.

import { h } from "../widgets.js";
import { addDevice, addTrack, tracks, uniqueId } from "../tracks.js";

export default function browser(el, ctx) {
  const { client } = ctx;
  const fail = (e) => ctx.toast(e.message);
  const devices = Object.values(client.state.devices).filter((d) => d.kind !== "output");
  const components = Object.entries(client.state.project.ui.components ?? {});

  async function add(kind) {
    const sel = ctx.selection;
    try {
      if (sel.track !== null) return await addDevice(client, sel.track, kind);
      if (!tracks(client).length) {
        const index = await addTrack(client, kind);
        return ctx.select({ track: index });
      }
      // Nothing selected: a free-standing node, to patch by hand in the graph.
      const id = uniqueId(client, kind);
      await client.tx([{ op: "add_node", id, kind }], `add ${id}`);
      ctx.select({ node: id });
    } catch (e) {
      fail(e);
    }
  }

  const target =
    ctx.selection.track !== null ? `to ${tracks(client)[ctx.selection.track]?.name}` : ctx.selection.node ? "as a free node" : "";

  el.append(
    h(
      "div",
      { class: "browser" },
      h("div", { class: "section-head" }, "Tracks"),
      h("div", { class: "row-buttons" },
        h("button", { onclick: () => addTrack(client, "osc").then((i) => ctx.select({ track: i }), fail) }, "+ Synth track"),
        h("button", { onclick: () => addTrack(client, "noise").then((i) => ctx.select({ track: i }), fail) }, "+ Noise track"),
        h("button", { onclick: () => addTrack(client, null).then((i) => ctx.select({ track: i }), fail) }, "+ Empty"),
      ),
      h("div", { class: "section-head" }, "Devices ", h("span", { class: "muted" }, target)),
      devices.map((d) =>
        h(
          "button",
          { class: "item", title: d.description, onclick: () => add(d.kind) },
          h("span", { class: "item-name" }, d.kind),
          h("span", { class: "item-desc" }, d.description),
        ),
      ),
      h("div", { class: "section-head" }, "Custom panels"),
      components.length
        ? components.map(([name, c]) =>
            h(
              "button",
              {
                class: "item",
                title: "Edit its source",
                onclick: () => ctx.editJson(`/ui/components/${name}/source`, { title: `Source of ${name} (Ctrl+Enter to save)` }),
              },
              h("span", { class: "item-name" }, name),
              h("span", { class: "item-desc" }, c.description ?? ""),
            ),
          )
        : h("div", { class: "muted pad" }, "None yet"),
      h("button", { class: "item add", onclick: ctx.actions["ui.newComponent"] }, "+ New custom panel"),
      h("div", { class: "section-head" }, "Project"),
      h(
        "div",
        { class: "row-buttons" },
        h("button", {
          onclick: () => {
            const path = prompt("Save project to (path on the machine running Icarus):", "song.icarus.json");
            if (path) client.request({ type: "save", path }).then(() => ctx.toast(`Saved ${path}`), fail);
          },
        }, "Save"),
        h("button", {
          onclick: () => {
            const path = prompt("Load project from:", "song.icarus.json");
            if (path) client.request({ type: "load", path }).catch(fail);
          },
        }, "Load"),
      ),
    ),
  );
}
