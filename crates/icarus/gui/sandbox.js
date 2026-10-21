// Custom panels: JavaScript stored at /ui/components/<type>/source, written by anyone.
//
// Each runs in a sandboxed iframe with no access to this page, so a component can do exactly
// what any protocol client can do and nothing more. Inside, the component's code gets:
//
//   root            the element to draw into (document.body)
//   icarus.request(req)        any protocol request; resolves with the result
//   icarus.get(path) / icarus.set(path, value) / icarus.gesture(g)
//   icarus.on(fn)              called with every change event
//   icarus.state               { project, devices, touches, transport }, kept current
//   icarus.props, icarus.panel this panel's props and id (/ui/panels/<id>)
//
// Requests are attributed to "<gui client>/<panel id>", so history shows which panel did what.

const BOOT = `
const pending = new Map();
let nextId = 0;
const listeners = [];
const icarus = {
  ...__INIT__,
  request(request) {
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve, reject });
      parent.postMessage({ icarus: "request", id, request }, "*");
    });
  },
  get(path) { return this.request({ type: "get", path }); },
  set(path, value) { return this.request({ type: "set", path, value }); },
  gesture(gesture) { return this.request({ type: "gesture", gesture }); },
  on(fn) { listeners.push(fn); },
};
const root = document.body;
addEventListener("message", (e) => {
  if (e.source !== parent) return;
  const m = e.data;
  if (m.icarus === "reply") {
    const w = pending.get(m.id);
    if (!w) return;
    pending.delete(m.id);
    m.ok ? w.resolve(m.result) : w.reject(new Error(m.error));
  } else if (m.icarus === "event") {
    icarus.state = m.state;
    for (const fn of listeners) { try { fn(m.event); } catch (err) { console.error(err); } }
  }
});
addEventListener("error", (e) => {
  document.body.insertAdjacentHTML("beforeend", '<pre class="icarus-error"></pre>');
  document.body.lastChild.textContent = String(e.message);
});
`;

function snapshot(client) {
  const { project, devices, touches, transport } = client.state;
  return { project, devices, touches, transport };
}

export function mountComponent(el, ctx, panel, id, component) {
  const theme = Object.entries(ctx.client.state.project.ui.theme ?? {})
    .map(([k, v]) => `${k}: ${v};`)
    .join(" ");
  const init = JSON.stringify({ state: snapshot(ctx.client), props: panel.props ?? {}, panel: id });
  // Keep the component's text from closing the script element early.
  const source = String(component.source ?? "").replace(/<\/script/gi, "<\\/script");
  const iframe = document.createElement("iframe");
  iframe.className = "component";
  iframe.setAttribute("sandbox", "allow-scripts");
  iframe.srcdoc = `<!doctype html><html><head><meta charset="utf-8"><style>
    :root { ${theme} }
    html, body { margin: 0; height: 100%; background: var(--panel); color: var(--text);
      font: 12px var(--font); overflow: hidden; position: relative; }
    .icarus-error { color: var(--danger); white-space: pre-wrap; padding: 8px; }
  </style></head><body><script>${BOOT.replace("__INIT__", init)}</script><script>${source}</script></body></html>`;
  el.append(iframe);

  const onMessage = async (e) => {
    if (e.source !== iframe.contentWindow || e.data?.icarus !== "request") return;
    const { id: reqId, request } = e.data;
    try {
      const result = await ctx.client.request(request, `${ctx.client.name}/${id}`);
      iframe.contentWindow?.postMessage({ icarus: "reply", id: reqId, ok: true, result }, "*");
    } catch (err) {
      iframe.contentWindow?.postMessage({ icarus: "reply", id: reqId, ok: false, error: err.message }, "*");
    }
  };
  window.addEventListener("message", onMessage);

  return {
    update(msg) {
      if (msg.type === "event") {
        iframe.contentWindow?.postMessage({ icarus: "event", event: msg.event, state: snapshot(ctx.client) }, "*");
      }
    },
    destroy() {
      window.removeEventListener("message", onMessage);
    },
  };
}

export const TEMPLATE = `// A custom panel. Draw into \`root\`; talk to Icarus through \`icarus\`.
// See sandbox.js for the full API. Everything here is undoable and attributed.

root.innerHTML = '<div style="padding:10px"><button id="b">Randomize first node</button><pre id="out"></pre></div>';
document.getElementById("b").onclick = async () => {
  const [id, node] = Object.entries(icarus.state.project.nodes).find(([id]) => id !== "master") ?? [];
  if (!id) return;
  const params = icarus.state.devices[node.kind].params;
  await icarus.request({
    type: "transaction",
    label: "randomize " + id,
    commands: params.map((p) => ({ op: "set_param", node: id, param: p.id, value: p.min + Math.random() * (p.max - p.min) })),
  });
};
icarus.on((event) => {
  document.getElementById("out").textContent = event.client + ": " + event.label;
});
`;
