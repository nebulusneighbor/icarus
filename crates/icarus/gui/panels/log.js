// Activity: every change from every client, newest first. Click a row for its paths.

import { clientColor, h } from "../widgets.js";

export default function log(el, ctx, panel, id) {
  const state = (ctx.panelState[id] ??= { filter: "", open: new Set() });
  const list = h("div", { class: "log" });
  const filter = h("input", {
    placeholder: "filter by client, label or path",
    value: state.filter,
    oninput: (e) => {
      state.filter = e.target.value;
      draw();
    },
  });
  el.append(h("div", { class: "toolbar" }, filter), list);

  function matches(e) {
    const f = state.filter.toLowerCase();
    return !f || e.client.toLowerCase().includes(f) || e.label.toLowerCase().includes(f) || e.changes.some((c) => c.path.includes(f));
  }

  // Consecutive identical actions (a gesture's stream of moves) collapse into one row.
  function grouped() {
    const rows = [];
    for (const e of ctx.log.filter(matches).slice(-500)) {
      const last = rows.at(-1);
      if (last && last.client === e.client && last.label === e.label) {
        last.count++;
        last.changes = e.changes;
        last.at = e.at;
      } else {
        rows.push({ ...e, count: 1 });
      }
    }
    return rows.slice(-200).reverse();
  }

  function draw() {
    const rows = grouped();
    list.replaceChildren(
      ...rows.map((e) => {
        const open = state.open.has(e.seq);
        return h(
          "div",
          {
            class: "log-row",
            onclick: () => {
              open ? state.open.delete(e.seq) : state.open.add(e.seq);
              draw();
            },
          },
          h("span", { class: "muted" }, e.at.toLocaleTimeString()),
          h("span", { class: "who", style: { "--c": clientColor(e.client.split("/")[0]) } }, e.client),
          h("span", {}, e.label, e.count > 1 ? h("span", { class: "muted" }, ` ×${e.count}`) : null),
          h("span", { class: "muted" }, `${e.changes.length} change${e.changes.length === 1 ? "" : "s"}`),
          open
            ? h(
                "div",
                { class: "log-changes" },
                e.changes.map((c) => h("div", {}, h("code", {}, c.path), " = ", h("code", {}, JSON.stringify(c.value)?.slice(0, 200)))),
              )
            : null,
        );
      }),
    );
  }
  draw();
  return { update: (msg) => msg.type === "event" && draw() };
}
