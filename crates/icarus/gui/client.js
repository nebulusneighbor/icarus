// Connection to an Icarus session over WebSocket, and a live mirror of its state.
//
// The GUI has no private API: everything below is ordinary protocol requests, the same ones
// agents, OSC surfaces and scripts send. The mirror is loaded once, then kept current by
// subscribing to every path and applying change events.

export class Client {
  constructor(url, name) {
    this.url = url;
    this.name = name;
    this.pending = new Map();
    this.nextId = 0;
    this.listeners = new Set();
    this.status = "connecting";
    this.state = {
      project: null,
      devices: {},
      touches: {},
      transport: { playing: false, live: false, position_s: 0 },
    };
  }

  /// Resolves once the first snapshot is loaded; reconnects forever after that.
  connect() {
    return new Promise((resolve) => {
      this.onFirstLoad = resolve;
      this.open();
    });
  }

  open() {
    const ws = new WebSocket(this.url);
    this.ws = ws;
    ws.onopen = async () => {
      await this.load();
      this.status = "open";
      this.emit({ type: "status" });
      this.onFirstLoad?.();
      this.onFirstLoad = null;
    };
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data);
      if (msg.event) return this.apply(msg.event);
      const waiter = this.pending.get(msg.id);
      if (!waiter) return;
      this.pending.delete(msg.id);
      msg.ok ? waiter.resolve(msg.result) : waiter.reject(new Error(msg.error));
    };
    ws.onclose = () => {
      this.status = "closed";
      for (const w of this.pending.values()) w.reject(new Error("disconnected"));
      this.pending.clear();
      this.emit({ type: "status" });
      setTimeout(() => this.open(), 1000);
    };
  }

  request(request, client = this.name) {
    return new Promise((resolve, reject) => {
      if (this.ws?.readyState !== WebSocket.OPEN) return reject(new Error("not connected"));
      const id = ++this.nextId;
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, client, request }));
    });
  }

  get(path) {
    return this.request({ type: "get", path });
  }
  set(path, value) {
    return this.request({ type: "set", path, value });
  }
  tx(commands, label) {
    return this.request({ type: "transaction", label, commands });
  }
  gesture(gesture) {
    return this.request({ type: "gesture", gesture });
  }

  on(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }
  emit(msg) {
    for (const fn of this.listeners) fn(msg);
  }

  async load() {
    const [project, devices, touches, transport] = await Promise.all([
      this.request({ type: "snapshot" }),
      this.request({ type: "list_devices" }),
      this.get("/touches"),
      this.get("/transport"),
    ]);
    Object.assign(this.state, {
      project,
      devices: Object.fromEntries(devices.map((d) => [d.kind, d])),
      touches,
      transport,
    });
    await this.request({ type: "subscribe", paths: ["/"] });
    this.emit({ type: "reload" });
  }

  /// Applies a change event to the mirror. `structural` tells the GUI to rebuild rather
  /// than just refresh controls in place.
  apply(event) {
    const { project, touches, transport } = this.state;
    let structural = false;
    for (const c of event.changes) {
      const seg = c.path.split("/").filter(Boolean);
      switch (seg[0]) {
        case "project":
          project[seg[1]] = c.value;
          structural = true;
          break;
        case "connections":
          project.connections = c.value;
          structural = true;
          break;
        case "nodes": {
          const id = seg[1];
          let node = project.nodes[id];
          if (seg[2] === "kind") {
            if (c.value === null) delete project.nodes[id];
            else if (node) node.kind = c.value;
            else project.nodes[id] = { kind: c.value, params: {}, automation: {} };
            structural = true;
            break;
          }
          if (!node) {
            if (c.value === null) break;
            node = project.nodes[id] = { kind: "", params: {}, automation: {} };
            structural = true;
          }
          const bucket = seg[2] === "params" ? (node.params ??= {}) : (node.automation ??= {});
          if (c.value === null) delete bucket[seg[3]];
          else bucket[seg[3]] = c.value;
          if (seg[2] === "automation") structural = true;
          break;
        }
        case "ui":
          setPointer(project.ui, seg.slice(1), c.value);
          structural = true;
          break;
        case "touches":
          if (c.value === null) delete touches[`${seg[1]}/${seg[2]}`];
          else touches[`${seg[1]}/${seg[2]}`] = c.value;
          break;
        case "transport":
          transport[seg[1]] = c.value;
          break;
      }
    }
    this.emit({ type: "event", event, structural });
  }

  // Parameter helpers. The curve maths mirrors ParamSpec in icarus-engine.
  spec(node, param) {
    const kind = this.state.project.nodes[node]?.kind;
    return this.state.devices[kind]?.params.find((p) => p.id === param);
  }
  value(node, param) {
    const v = this.state.project.nodes[node]?.params?.[param];
    return v ?? this.spec(node, param)?.default;
  }
  heldBy(node, param) {
    return this.state.touches[`${node}/${param}`];
  }
}

export function toNorm(spec, v) {
  v = Math.min(spec.max, Math.max(spec.min, v));
  return spec.curve === "log"
    ? Math.log(v / spec.min) / Math.log(spec.max / spec.min)
    : (v - spec.min) / (spec.max - spec.min);
}

export function fromNorm(spec, n) {
  n = Math.min(1, Math.max(0, n));
  const v = spec.curve === "log" ? spec.min * (spec.max / spec.min) ** n : spec.min + n * (spec.max - spec.min);
  return spec.curve === "stepped" ? Math.round(v) : v;
}

/// Mirrors the session's set_pointer: creates objects on the way, `null` deletes.
export function setPointer(root, segs, value) {
  if (segs.length === 0) return;
  let cur = root;
  for (const s of segs.slice(0, -1)) {
    if (cur[s] === undefined || cur[s] === null || typeof cur[s] !== "object") cur[s] = {};
    cur = cur[s];
  }
  const last = segs.at(-1);
  if (value === null) {
    if (Array.isArray(cur)) cur.splice(Number(last), 1);
    else delete cur[last];
  } else {
    cur[last] = value;
  }
}
