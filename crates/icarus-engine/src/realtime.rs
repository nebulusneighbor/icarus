use crate::{Graph, MAX_BLOCK, ParamAddr};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Messages from the control thread to the audio thread.
pub enum EngineMsg {
    SetParam(ParamAddr, f32),
    /// Replace the running graph. The old one is sent back so it is freed off the audio thread.
    Swap(Box<Graph>),
    /// Start, or stop and rewind.
    SetPlaying(bool),
}

/// Lives on the audio thread. Never allocates, locks or frees.
pub struct RealtimeEngine {
    graph: Box<Graph>,
    rx: rtrb::Consumer<EngineMsg>,
    trash: rtrb::Producer<Box<Graph>>,
    pos: u64,
    playing: bool,
    position: Arc<AtomicU64>,
}

/// Lives on the control thread; talks to a [`RealtimeEngine`].
pub struct EngineHandle {
    tx: rtrb::Producer<EngineMsg>,
    trash: rtrb::Consumer<Box<Graph>>,
    addrs: BTreeMap<(String, String), ParamAddr>,
    position: Arc<AtomicU64>,
}

pub fn realtime_pair(graph: Graph) -> (RealtimeEngine, EngineHandle) {
    let (tx, rx) = rtrb::RingBuffer::new(1024);
    let (trash_tx, trash_rx) = rtrb::RingBuffer::new(16);
    let addrs = graph.param_addrs();
    let position = Arc::new(AtomicU64::new(0));
    (
        RealtimeEngine {
            graph: Box::new(graph),
            rx,
            trash: trash_tx,
            pos: 0,
            playing: false,
            position: position.clone(),
        },
        EngineHandle {
            tx,
            trash: trash_rx,
            addrs,
            position,
        },
    )
}

impl RealtimeEngine {
    pub fn sample_rate(&self) -> u32 {
        self.graph.sample_rate()
    }

    /// Fills interleaved stereo `out`, looping over the project length.
    pub fn render_interleaved(&mut self, out: &mut [f32]) {
        while let Ok(msg) = self.rx.pop() {
            match msg {
                EngineMsg::SetParam(addr, value) => self.graph.set_param(addr, value),
                EngineMsg::Swap(graph) => {
                    let old = std::mem::replace(&mut self.graph, graph);
                    if let Err(rtrb::PushError::Full(old)) = self.trash.push(old) {
                        // Leaking beats freeing on the audio thread; the trash ring is drained
                        // on every control-side send, so this only happens if that thread stalls.
                        std::mem::forget(old);
                    }
                    self.pos %= self.graph.length();
                }
                EngineMsg::SetPlaying(playing) => {
                    self.playing = playing;
                    if !playing {
                        self.pos = 0;
                    }
                }
            }
        }
        if !self.playing {
            out.fill(0.0);
            self.position.store(self.pos, Ordering::Relaxed);
            return;
        }

        let mut frames_left = out.len() / 2;
        let mut offset = 0;
        while frames_left > 0 {
            let until_loop = (self.graph.length() - self.pos) as usize;
            let n = frames_left.min(MAX_BLOCK).min(until_loop);
            self.graph.process(self.pos, n);
            let (l, r) = self.graph.output(n);
            for i in 0..n {
                out[2 * (offset + i)] = l[i];
                out[2 * (offset + i) + 1] = r[i];
            }
            self.pos = (self.pos + n as u64) % self.graph.length();
            offset += n;
            frames_left -= n;
        }
        self.position.store(self.pos, Ordering::Relaxed);
    }
}

impl EngineHandle {
    /// Returns false if the parameter does not exist or the queue is full.
    pub fn set_param(&mut self, node: &str, param: &str, value: f64) -> bool {
        self.drain_trash();
        match self.addrs.get(&(node.to_string(), param.to_string())) {
            Some(&addr) => self.tx.push(EngineMsg::SetParam(addr, value as f32)).is_ok(),
            None => false,
        }
    }

    pub fn swap(&mut self, graph: Graph) -> bool {
        self.drain_trash();
        let addrs = graph.param_addrs();
        let ok = self.tx.push(EngineMsg::Swap(Box::new(graph))).is_ok();
        if ok {
            self.addrs = addrs;
        }
        ok
    }

    pub fn set_playing(&mut self, playing: bool) -> bool {
        self.drain_trash();
        self.tx.push(EngineMsg::SetPlaying(playing)).is_ok()
    }

    /// Playback position in samples, as of the last audio callback.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    fn drain_trash(&mut self) {
        while self.trash.pop().is_ok() {}
    }
}
