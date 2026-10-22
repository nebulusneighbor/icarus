# Icarus

An open, modular DAW for Linux, built from scratch so that **agents, people, controllers and extensible rigs are all equal clients**.

Every part of the signal chain is raw, addressable and changeable through one protocol. No client has a private API, not even the GUI. An agent can do anything a person can, with the same fingers: it touches a knob, moves it, hears the result live, and lets go. It can also experiment before touching anything: it forks the project, renders dozens of variants in parallel, and measures what each parameter does to the sound.

> **Status: early prototype.** The core works: a deterministic engine, live PipeWire playback, transactions, gestures, undo with attribution, the path tree, change events with subscriptions, offline forked rendering and probing, MCP, OSC, WebSocket and JSON-socket adapters, and a stock GUI that is itself rewritable data. There's no MIDI or sequencer yet. See [ROADMAP](#roadmap).

![The stock Icarus GUI: transport, browser, mixer, device chain, inspector and activity](docs/gui.png)

## Try it

```sh
cargo build --release
./target/release/icarus gui        # opens the GUI in your browser, with live audio
```

Add a synth track, press play (space), and turn knobs. To let Claude Code into the **same** session, so you can both work on it at once:

```sh
claude mcp add icarus -- /path/to/icarus mcp --connect 127.0.0.1:7878
```

Its hand shows on the knobs it's holding, its changes appear in Activity under its own name, and Ctrl+Z undoes whatever was last done, by anyone.

There's also a headless demo:

```sh
./target/release/icarus demo
```

The demo builds a saw → filter → gain patch, records a filter sweep as a gesture, renders it to `icarus-demo.wav`, proves renders are bit-identical across block sizes, and probes 15 forked variants in parallel to rank which control moves which feature:

```
5. Probe: 5 cutoffs x 3 resonances on forked copies, rendered in parallel
   15 variants in 37 ms. Strongest effects:
   filt.cutoff      -> centroid_hz    +3208.5 over full range
   filt.resonance   -> centroid_hz     +425.7 over full range
```

## The GUI is data, so anyone can change it

The stock GUI is a browser app served by Icarus itself. It's an ordinary protocol client with no private access, and everything about it lives in the project at `/ui`:

| Path | What it controls |
|---|---|
| `/ui/layout` | The grid: `columns`, `rows`, and named `areas` |
| `/ui/panels/<id>` | Each panel: `type`, which `area` it sits in, `title`, `order`, `props` |
| `/ui/tracks` | How the graph is read as tracks: `{name, color, devices: [node ids], mix}` |
| `/ui/theme` | Every colour, font and radius (CSS variables) |
| `/ui/keys` | Key bindings, e.g. `"ctrl+z": "undo"` |
| `/ui/components/<type>` | **Code** for new panel types, written by people or agents |
| `/ui/graph/positions` | Where nodes sit in the Graph view |

Change any of it from anywhere, and every open GUI rebuilds:

```jsonc
{"type": "set", "path": "/ui/theme/--accent", "value": "#7ee787"}            // restyle
{"type": "set", "path": "/ui/panels/log/area", "value": "right"}             // rearrange
{"type": "set", "path": "/ui/layout/columns", "value": "160px 1fr 0px"}      // hide a column
{"type": "transaction", "commands": [                                        // invent a panel
  {"op": "set_ui", "path": "/components/meter", "value": {"source": "root.textContent = '…'"}},
  {"op": "set_ui", "path": "/panels/meter", "value": {"type": "meter", "area": "bottom"}}]}
```

Custom panels run in a sandboxed iframe and can do exactly what any client can (`icarus.request/get/set/gesture/on`) and nothing more. Their requests are attributed to the panel, for example `gui:human/xy`. The stock XY pad ([`xy-pad.js`](crates/icarus-model/ui/xy-pad.js)) is one of these: it configures itself by writing its own props.

In the GUI, **Edit UI** (Ctrl+E) exposes the same thing to people: add, move and remove panels, edit the layout, theme and keys as JSON, or write a new panel and see it live. The **Inspector** shows the whole session as the path tree, and you can edit any value there. To hack on the built-in panels themselves, run `icarus gui --gui-dir crates/icarus/gui` and reload the page after each edit.

The built-in panels are Transport, Browser, Mixer, Devices, Graph (patch anything to anything), Automation, Inspector, Activity and XY Pad.

## Connect anything

One `serve` process runs one session. Turn on as many adapters as you like; they all share it.

```sh
icarus serve --live --mcp --osc 9000 --ws 127.0.0.1:7879 --socket 127.0.0.1:7878 [--project song.json]
```

| Adapter | For | Docs |
|---|---|---|
| `--mcp` | Claude Code and any other MCP-capable agent | [`crates/icarus/src/mcp.rs`](crates/icarus/src/mcp.rs) |
| `--osc <port>` | TouchOSC, VR (Unity/Unreal/Godot OSC), Max/Pd, hardware | [`crates/icarus/src/osc.rs`](crates/icarus/src/osc.rs) |
| `--ws <addr>` | The GUI, browsers, WebXR, game engines (HTTP and WebSocket on one port) | [`crates/icarus/src/ws.rs`](crates/icarus/src/ws.rs) |
| `--socket <addr>` | Scripts, custom agents: line-delimited JSON | [`crates/icarus/src/socket.rs`](crates/icarus/src/socket.rs) |
| `--live` | Hear every change in real time through PipeWire | [`crates/icarus-pipewire`](crates/icarus-pipewire) |

**Claude Code**, in its own session, or bridged into a running one (`icarus gui` opens a socket on 127.0.0.1:7878):

```sh
claude mcp add icarus -- /path/to/icarus mcp                          # its own session
claude mcp add icarus -- /path/to/icarus mcp --connect 127.0.0.1:7878 # join the GUI's session
```

You can then ask it, for example, to "build a bass patch, probe the filter, and sweep it in over 4 seconds" while you play with the same controls from the GUI or TouchOSC.

## Everything is a path

```
/project/{seed,tempo_bpm,length_s}
/nodes/<id>                              {"kind": "filter"} adds, null removes
/nodes/<id>/params/<param>               plain units (Hz, dB)
/nodes/<id>/params/<param>/normalized    0..1, for faders and VR hands
/nodes/<id>/params/<param>/spec          range, curve, unit, what it does to the sound
/nodes/<id>/automation/<param>
/connections
/devices/<kind>
/ui/...                                  the GUI itself: layout, panels, tracks, theme, keys, components
/touches                                 who is holding which control right now
/transport/{playing,live,position_s}     set playing to true to start playback
```

Over the socket, `{"type":"set","path":"/nodes/filt/params/cutoff","value":800}`. Over OSC, `/icarus/nodes/filt/params/cutoff/normalized 0.4`. Over MCP, the `set` tool. Every write becomes validated, undoable commands attributed to the client that sent it:

```json
{"undo": [{"client": "osc:192.168.1.20:9001", "label": "gesture filt.cutoff"},
          {"client": "mcp:claude-code",       "label": "add reverb send"}]}
```

## Watch everything

Any client can subscribe to path prefixes and get every change pushed to it, whoever made it:

```jsonc
// → over WebSocket or the JSON socket
{"id": 1, "type": "subscribe", "paths": ["/nodes/filt", "/touches"]}
// ← while an agent sweeps the filter from MCP
{"event": {"seq": 2, "client": "mcp:claude-code", "label": "touch filt.cutoff",
           "changes": [{"path": "/touches/filt/cutoff", "value": "mcp:claude-code"}]}}
{"event": {"seq": 3, "client": "mcp:claude-code", "label": "move filt.cutoff",
           "changes": [{"path": "/nodes/filt/params/cutoff", "value": 2000.0, "normalized": 0.667}]}}
{"event": {"seq": 4, "client": "mcp:claude-code", "label": "release filt.cutoff",
           "changes": [{"path": "/touches/filt/cutoff", "value": null}]}}
```

`/touches` is presence: it shows whose hand is on which control, so a VR scene can draw the agent's hand on the knob. OSC surfaces send `/icarus/subscribe /nodes/filt [reply_port]` and get `/icarus/nodes/filt/params/cutoff 2000` plus `.../normalized 0.667` back, which motorised faders and TouchOSC need. Agents on MCP use the `subscribe` and `events` tools.

## Why determinism matters

The same project always renders to **bit-identical** audio, whatever the block size, machine or thread count. Parameters resolve per frame from the absolute sample position, every random source is seeded from the project, and devices hold no wall-clock state. That's what makes **forked rendering** useful: when an agent renders two copies that differ in one knob, any difference in the output is caused by that knob. Tests in [`icarus-render`](crates/icarus-render/src/lib.rs) enforce it.

## Layout

| Crate | Role |
|---|---|
| `icarus-model` | Project document and the protocol (`Request`, `Command`, `Gesture`). Plain data. |
| `icarus-engine` | Deterministic audio graph, devices, parameter specs, lock-free realtime runner |
| `icarus-analysis` | Features that describe audio to a model: levels, crest, centroid, band balance, width |
| `icarus-render` | Offline forked rendering, taps on any node, parallel probing and sensitivity |
| `icarus-session` | Transactions, gestures, undo with attribution, the path tree, change events and subscriptions |
| `icarus-pipewire` | Live output |
| `icarus` | CLI, adapters (MCP, OSC, WebSocket, JSON socket), and the stock GUI in `gui/` |

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the design rules and how to extend each layer.

## Roadmap

- **Live meters and taps as subscribable paths** (levels and spectra streamed to VR and agents).
- **Authentication** for the network adapters.
- **Clips, MIDI and a sequencer**, plus a clip grid and piano roll in the GUI.
- **Device plugins:** CLAP and LV2 hosting out of process, and WASM/Faust devices that agents can write and hot-load.
- **State-preserving graph swaps** (today a structural edit resets DSP state and can click).
- **Differentiable twins** of built-in devices, for gradient-based matching ("make it sound like this reference").
- **Semantic hearing:** audio-text embeddings alongside the DSP features.
- **WebXR GUI** on the same `/ui` document.

## License

GPL-3.0-or-later. The core stays open, and so do its forks.
