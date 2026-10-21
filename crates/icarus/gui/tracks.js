// Tracks are an interpretation of the graph, not an engine concept. A track in /ui/tracks
// names a chain of nodes, `devices[0] -> devices[1] -> ... -> mix -> master`, where `mix` is a
// gain node used as the fader. Edit /ui/tracks to reinterpret the graph differently.
//
// Every operation is one transaction that changes the graph and /ui/tracks together, so it
// undoes in one step.

const COLORS = ["#f0a236", "#4fb3d9", "#9bd45a", "#e5699a", "#b48cf2", "#58d1b2", "#e5534b", "#d9c14f"];

export const tracks = (client) => client.state.project.ui.tracks ?? [];

export function uniqueId(client, base) {
  const nodes = client.state.project.nodes;
  if (!nodes[base]) return base;
  for (let i = 2; ; i++) if (!nodes[`${base}${i}`]) return `${base}${i}`;
}

const edges = (chain) => chain.slice(0, -1).map((from, i) => ({ from, to: chain[i + 1] }));
const key = (e) => `${e.from}->${e.to}`;

/// Commands that rewire a track from one device list to another.
function rewire(client, track, devices) {
  const before = edges([...track.devices, track.mix]);
  const after = edges([...devices, track.mix]);
  const existing = new Set(client.state.project.connections.map(key));
  const keep = new Set(after.map(key));
  const had = new Set(before.map(key));
  return [
    ...before.filter((e) => !keep.has(key(e)) && existing.has(key(e))).map((e) => ({ op: "disconnect", ...e })),
    ...after.filter((e) => !had.has(key(e)) || !existing.has(key(e))).map((e) => ({ op: "connect", ...e })),
  ];
}

export function addTrack(client, instrument = "osc") {
  const list = tracks(client);
  const n = list.length + 1;
  const id = uniqueId(client, `t${n}`);
  const mix = uniqueId(client, `${id}_mix`);
  const inst = instrument ? uniqueId(client, `${id}_${instrument}`) : null;
  const devices = inst ? [inst] : [];
  const track = { id, name: `Track ${n}`, color: COLORS[(n - 1) % COLORS.length], devices, mix };
  const commands = [
    { op: "add_node", id: mix, kind: "gain", params: { gain: -6 } },
    { op: "connect", from: mix, to: "master" },
  ];
  if (inst) commands.push({ op: "add_node", id: inst, kind: instrument }, { op: "connect", from: inst, to: mix });
  commands.push({ op: "set_ui", path: `/tracks/${list.length}`, value: track });
  return client.tx(commands, `add ${track.name}`).then(() => list.length);
}

export function addDevice(client, index, kind) {
  const track = tracks(client)[index];
  const id = uniqueId(client, `${track.id}_${kind}`);
  const devices = [...track.devices, id];
  return client.tx(
    [
      { op: "add_node", id, kind },
      ...rewire(client, track, devices),
      { op: "set_ui", path: `/tracks/${index}/devices`, value: devices },
    ],
    `add ${kind} to ${track.name}`,
  );
}

export function removeDevice(client, index, id) {
  const track = tracks(client)[index];
  const devices = track.devices.filter((d) => d !== id);
  // Removing the node drops its connections; rewire the neighbours around the gap.
  const commands = [{ op: "remove_node", id }];
  const existing = new Set(client.state.project.connections.filter((c) => c.from !== id && c.to !== id).map(key));
  for (const e of edges([...devices, track.mix])) if (!existing.has(key(e))) commands.push({ op: "connect", ...e });
  commands.push({ op: "set_ui", path: `/tracks/${index}/devices`, value: devices });
  return client.tx(commands, `remove ${id}`);
}

export function moveDevice(client, index, id, delta) {
  const track = tracks(client)[index];
  const devices = [...track.devices];
  const i = devices.indexOf(id);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= devices.length) return Promise.resolve();
  [devices[i], devices[j]] = [devices[j], devices[i]];
  return client.tx(
    [...rewire(client, track, devices), { op: "set_ui", path: `/tracks/${index}/devices`, value: devices }],
    `reorder ${track.name}`,
  );
}

export function deleteTrack(client, index) {
  const track = tracks(client)[index];
  const nodes = [...track.devices, track.mix].filter((id) => client.state.project.nodes[id]);
  return client.tx(
    [...nodes.map((id) => ({ op: "remove_node", id })), { op: "set_ui", path: `/tracks/${index}`, value: null }],
    `delete ${track.name}`,
  );
}

/// The track a node belongs to, if any.
export function trackOf(client, node) {
  return tracks(client).findIndex((t) => t.mix === node || t.devices.includes(node));
}
