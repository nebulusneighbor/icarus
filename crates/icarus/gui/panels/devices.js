// Device chain of the selected track, or the selected node on its own.

import { h, knob } from "../widgets.js";
import { addDevice, moveDevice, removeDevice, tracks } from "../tracks.js";

function deviceCard(ctx, id, { onLeft, onRight, onRemove } = {}) {
  const { client } = ctx;
  const node = client.state.project.nodes[id];
  if (!node) return h("div", { class: "device missing" }, `missing node ${id}`);
  const info = client.state.devices[node.kind];
  return h(
    "div",
    { class: `device${ctx.selection.node === id ? " selected" : ""}` },
    h(
      "div",
      { class: "device-head", title: info?.description },
      h("span", { class: "device-kind" }, node.kind),
      h("span", { class: "device-id muted" }, id),
      h("span", { class: "spacer" }),
      onLeft ? h("button", { class: "icon", title: "Move earlier", onclick: onLeft }, "◀") : null,
      onRight ? h("button", { class: "icon", title: "Move later", onclick: onRight }, "▶") : null,
      h("button", { class: "icon", title: "Inspect raw paths", onclick: () => ctx.editJson(`/nodes/${id}`, { title: `${id} (read-only view; edit params with knobs or paths)` }) }, "{}"),
      onRemove ? h("button", { class: "icon", title: "Remove", onclick: onRemove }, "✕") : null,
    ),
    h("div", { class: "knobs" }, (info?.params ?? []).map((p) => knob(ctx, id, p.id))),
  );
}

export default function devices(el, ctx) {
  const { client } = ctx;
  const fail = (e) => ctx.toast(e.message);
  const row = h("div", { class: "chain" });
  el.append(row);

  const index = ctx.selection.track;
  const track = index !== null ? tracks(client)[index] : null;
  if (track) {
    track.devices.forEach((id, i) => {
      row.append(
        deviceCard(ctx, id, {
          onLeft: i > 0 ? () => moveDevice(client, index, id, -1).catch(fail) : null,
          onRight: i < track.devices.length - 1 ? () => moveDevice(client, index, id, 1).catch(fail) : null,
          onRemove: () => removeDevice(client, index, id).catch(fail),
        }),
        h("div", { class: "arrow" }, "→"),
      );
    });
    const menu = h(
      "select",
      {
        class: "add-device",
        onchange: (e) => e.target.value && addDevice(client, index, e.target.value).catch(fail),
      },
      h("option", { value: "" }, "+ Add device"),
      Object.keys(client.state.devices)
        .filter((k) => k !== "output")
        .map((k) => h("option", { value: k }, k)),
    );
    row.append(menu, h("div", { class: "arrow" }, "→"), h("div", { class: "device ghost", style: { "--track": track.color } }, h("div", { class: "device-head" }, track.name, " mix")));
  } else if (ctx.selection.node && client.state.project.nodes[ctx.selection.node]) {
    const id = ctx.selection.node;
    row.append(
      deviceCard(ctx, id, {
        onRemove: id === "master" ? null : () => client.tx([{ op: "remove_node", id }], `remove ${id}`).catch(fail),
      }),
    );
  } else {
    row.append(h("div", { class: "empty" }, "Select a track in the Mixer or a node in the Graph to see its devices."));
  }
}
