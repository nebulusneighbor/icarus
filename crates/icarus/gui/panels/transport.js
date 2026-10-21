// Transport bar: play/stop, position, tempo, loop length, undo/redo, who is here, and the
// switch into edit mode where the GUI itself can be rearranged.

import { clientColor, h } from "../widgets.js";

export default function transport(el, ctx) {
  const { client } = ctx;
  const project = client.state.project;
  const t = client.state.transport;

  const play = h("button", { class: "play", onclick: ctx.actions["transport.toggle"] });
  const position = h("div", { class: "position" });
  const numberInput = (path, value, step, title) =>
    h("input", {
      type: "number",
      step,
      value,
      title,
      onchange: (e) => client.set(path, Number(e.target.value)).catch((err) => ctx.toast(err.message)),
    });
  const presence = h("div", { class: "presence" });
  const status = h("span", { class: "status" });
  const name = h("input", {
    class: "name",
    value: client.name.replace(/^gui:/, ""),
    title: "Your name. Your changes are attributed to it.",
    onchange: (e) => ctx.rename(e.target.value.trim() || "human"),
  });

  el.append(
    h(
      "div",
      { class: "transport" },
      h("strong", { class: "brand" }, "ICARUS"),
      play,
      position,
      h("label", {}, "BPM", numberInput("/project/tempo_bpm", project.tempo_bpm, 1, "Tempo")),
      h("label", {}, "Loop", numberInput("/project/length_s", project.length_s, 0.5, "Loop length in seconds"), "s"),
      h("button", { title: "Undo (Ctrl+Z)", onclick: ctx.actions.undo }, "↶"),
      h("button", { title: "Redo (Ctrl+Shift+Z)", onclick: ctx.actions.redo }, "↷"),
      h("span", { class: "spacer" }),
      ctx.editing
        ? [
            h("button", { onclick: ctx.actions["ui.addPanel"] }, "+ Panel"),
            h("button", { onclick: ctx.actions["ui.newComponent"] }, "+ Component"),
            h("button", { onclick: () => ctx.editJson("/ui/layout") }, "Layout"),
            h("button", { onclick: () => ctx.editJson("/ui/theme") }, "Theme"),
            h("button", { onclick: () => ctx.editJson("/ui/keys") }, "Keys"),
            h("button", { onclick: () => ctx.editJson("/ui") }, "All UI"),
          ]
        : null,
      presence,
      name,
      h("button", { class: ctx.editing ? "primary" : "", title: "Rearrange and rewrite the interface (Ctrl+E)", onclick: ctx.actions["ui.edit"] }, ctx.editing ? "Done" : "Edit UI"),
      status,
    ),
  );

  function update() {
    play.textContent = t.playing ? "■" : "▶";
    play.classList.toggle("on", t.playing);
    play.title = t.live ? "Play / stop (space)" : "No audio output: started with --no-live";
    const seconds = client.state.transport.position_s ?? 0;
    const beats = (seconds * project.tempo_bpm) / 60;
    position.textContent = `${Math.floor(beats / 4) + 1}.${Math.floor(beats % 4) + 1}  ·  ${seconds.toFixed(2)} s`;
    status.className = `status ${client.status}`;
    status.title = client.status === "open" ? "Connected" : "Disconnected, retrying";

    // Presence: whoever is holding a control, plus anyone active in the last 20 seconds.
    const now = Date.now();
    const active = new Map();
    for (const e of ctx.log) if (now - e.at < 20_000) active.set(e.client.split("/")[0], "");
    for (const [control, who] of Object.entries(client.state.touches)) active.set(who.split("/")[0], control);
    presence.replaceChildren(
      ...[...active].map(([who, control]) =>
        h("span", { class: "who", style: { "--c": clientColor(who) }, title: control ? `${who} is holding ${control}` : `${who} was active recently` }, who + (control ? " ✋" : "")),
      ),
    );
  }
  update();
  const timer = setInterval(update, 1000);
  return { update, destroy: () => clearInterval(timer) };
}
