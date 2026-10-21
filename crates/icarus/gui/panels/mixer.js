// Mixer: one strip per track in /ui/tracks, with the track's mix gain as the fader.

import { fader, h, knob } from "../widgets.js";
import { addTrack, deleteTrack, tracks } from "../tracks.js";

export default function mixer(el, ctx) {
  const { client } = ctx;
  const fail = (e) => ctx.toast(e.message);
  const strips = tracks(client).map((track, i) => {
    const hasMix = !!client.state.project.nodes[track.mix];
    return h(
      "div",
      {
        class: `strip${ctx.selection.track === i ? " selected" : ""}`,
        style: { "--track": track.color },
        onclick: (e) => !e.target.closest(".knob, .fader, button") && ctx.select({ track: i }),
      },
      h(
        "div",
        {
          class: "strip-name",
          title: "Double-click to rename",
          ondblclick: () => {
            const name = prompt("Track name", track.name);
            if (name) client.set(`/ui/tracks/${i}/name`, name).catch(fail);
          },
        },
        track.name,
      ),
      h("div", { class: "strip-chain muted" }, track.devices.length ? track.devices.map((d) => client.state.project.nodes[d]?.kind ?? "?").join(" → ") : "no devices"),
      hasMix ? knob(ctx, track.mix, "pan", { label: "Pan" }) : null,
      hasMix ? fader(ctx, track.mix, "gain") : h("div", { class: "muted" }, `missing ${track.mix}`),
      h(
        "button",
        {
          class: "icon",
          title: "Delete track",
          onclick: () => confirm(`Delete ${track.name} and its devices?`) && deleteTrack(client, i).catch(fail),
        },
        "✕",
      ),
    );
  });

  el.append(
    h(
      "div",
      { class: "mixer" },
      strips,
      h("div", { class: "strip master" }, h("div", { class: "strip-name" }, "Master"), h("div", { class: "muted" }, "output")),
      h("button", { class: "strip add", onclick: () => addTrack(client, "osc").then((i) => ctx.select({ track: i }), fail) }, "+ Track"),
    ),
  );
}
