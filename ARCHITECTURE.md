# Architecture

```
 Claude / any agent    TouchOSC / VR    scripts, game engines    (future) GUI
        │ MCP               │ OSC              │ JSON socket            │
        └───────────────────┴──────────┬───────┴────────────────────────┘
                                       ▼
                       icarus-session   (one per running project)
          transactions · gestures · undo/redo with attribution · path tree
                 │                                   │
        EngineSink (live)                    queries: render, probe
                 ▼                                   ▼
   icarus-pipewire ◄─ icarus-engine ─►  icarus-render ─► icarus-analysis
   (realtime thread)   (graph, devices)  (forked, offline, parallel)
```

## Rules

These hold for every change. If one has to bend, discuss it in an issue first.

1. **One door.** All changes go through `Session::handle(client, Request)`. Adapters translate; they never mutate the project themselves. If a client needs a capability, add it to the protocol so that every client gets it.
2. **Everything addressable.** New state gets a path in `icarus-session/src/paths.rs`, readable and (where it makes sense) writable, with `list` support so clients can discover it.
3. **Every write is a command.** Writes are validated, undoable and attributed to a client. Transactions are all or nothing.
4. **Determinism.** Engine output depends only on the project. No wall-clock time, no unseeded randomness, no `std` hashing in DSP, no block-size-dependent maths. Parameters are resolved per frame from the absolute sample position. The render tests must stay green.
5. **The audio thread never waits.** No locks, allocation or freeing on it. Graphs are compiled off-thread and swapped through a ring buffer, and old graphs are sent back to be freed.
6. **Self-describing.** Every parameter carries a `ParamSpec`: range, curve, unit, a semantic tag, and a description of what it does to the sound. Agents read these, so write them for a musician.
7. **Agents are clients, not components.** Icarus ships no AI. Any agent attaches through an adapter, several can attach at once, and none gets special powers.

## Extending

| You want to add | Where |
|---|---|
| A device | `icarus-engine/src/devices/`: implement `Processor`, give it a `PARAMS` table, register it in `DEVICES` and `create`. It must be deterministic. |
| An audio feature | `icarus-analysis`: add a field to `Features` and to `NAMES`/`values()`; probes rank it automatically. |
| A command | `icarus-model` `Command`, then `icarus-session/src/apply.rs`, then a path in `paths.rs` if it is state. |
| A transport (WebSocket, gRPC, MIDI, ...) | A module in `crates/icarus/src/` that turns its input into `Request`s, like `socket.rs`. Use a client name such as `ws:<peer>`. |
| An agent | No code needed: connect over MCP, OSC or the socket. If your agent needs something it can't do, that's a missing protocol feature, so open an issue. |

## Gestures

A gesture is `begin → move* → end` on one parameter, held by one client. While it's held, other clients can't move that parameter (like a finger on a motorised fader). The whole gesture is one undo step. With `record: true`, moves carrying `at_s` are written to automation. Live playback hears every move immediately, because parameter-only changes go straight to the audio thread without recompiling.

## Probing

`probe` clones the project once per combination of swept values, removes automation on the swept parameters in each fork, renders the forks in parallel, and measures a chosen node (any tap, not only the master). It returns every row, plus a sensitivity table: the least-squares change in each feature across each parameter's normalized range, strongest first. Because the engine is deterministic, a probe row equals a direct render of the same settings, and a test checks this.

## Known limits (prototype)

- Structural edits recompile the graph and reset DSP state, so you may hear a click.
- The session is behind one mutex, so a long probe blocks other clients until it finishes.
- Offline renders always start at sample 0 so that the state at the start of the region is correct, which means rendering late regions costs more.
- Devices are compiled in. Plugin hosting and hot-loaded devices are on the roadmap.
