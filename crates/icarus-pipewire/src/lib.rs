//! Realtime playback through PipeWire.
//!
//! [`LiveOutput`] is an [`EngineSink`]: attach it to a session and every change any client makes
//! is heard immediately. Parameter changes go straight to the audio thread; structural changes
//! are compiled on the control thread and swapped in whole.

use anyhow::{Context, anyhow};
use icarus_engine::{EngineHandle, Graph, RealtimeEngine, realtime_pair};
use icarus_model::Project;
use icarus_session::EngineSink;
use pipewire as pw;
use pw::{properties::properties, spa};

const CHANNELS: usize = 2;
const STRIDE: usize = CHANNELS * std::mem::size_of::<f32>();
/// Most frames rendered per PipeWire callback.
const MAX_FRAMES: usize = 8192;

pub struct LiveOutput {
    handle: EngineHandle,
}

impl LiveOutput {
    /// Starts a PipeWire playback stream for `project` on its own thread.
    pub fn start(project: &Project) -> anyhow::Result<Self> {
        let (engine, handle) = realtime_pair(Graph::compile(project)?);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("icarus-pipewire".into())
            .spawn(move || {
                if let Err(e) = run(engine, &ready_tx) {
                    let _ = ready_tx.send(Err(e));
                }
            })?;
        ready_rx.recv().context("PipeWire thread exited")??;
        Ok(Self { handle })
    }
}

impl EngineSink for LiveOutput {
    fn param_changed(&mut self, node: &str, param: &str, value: f64) {
        self.handle.set_param(node, param, value);
    }

    fn set_playing(&mut self, playing: bool) {
        self.handle.set_playing(playing);
    }

    fn position(&self) -> Option<u64> {
        Some(self.handle.position())
    }

    fn project_changed(&mut self, project: &Project) {
        match Graph::compile(project) {
            Ok(graph) => {
                self.handle.swap(graph);
            }
            Err(e) => eprintln!("icarus: live engine kept the previous graph: {e}"),
        }
    }
}

struct State {
    engine: RealtimeEngine,
    scratch: Vec<f32>,
}

fn run(engine: RealtimeEngine, ready: &std::sync::mpsc::Sender<anyhow::Result<()>>) -> anyhow::Result<()> {
    pw::init();
    let sample_rate = engine.sample_rate();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| anyhow!("PipeWire main loop: {e}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| anyhow!("PipeWire context: {e}"))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| anyhow!("cannot connect to PipeWire: {e}"))?;

    let stream = pw::stream::StreamBox::new(
        &core,
        "Icarus",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::AUDIO_CHANNELS => "2",
        },
    )?;

    let state = State {
        engine,
        scratch: vec![0.0; MAX_FRAMES * CHANNELS],
    };
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let requested = buffer.requested() as usize;
            let data = &mut buffer.datas_mut()[0];
            let frames = match data.data() {
                Some(bytes) => {
                    let capacity = bytes.len() / STRIDE;
                    let frames = if requested > 0 {
                        requested.min(capacity)
                    } else {
                        capacity
                    }
                    .min(MAX_FRAMES);
                    let out = &mut state.scratch[..frames * CHANNELS];
                    state.engine.render_interleaved(out);
                    for (dst, s) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(out.iter()) {
                        *dst = s.to_le_bytes();
                    }
                    frames
                }
                None => 0,
            };
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = STRIDE as _;
            *chunk.size_mut() = (STRIDE * frames) as _;
        })
        .register()?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(sample_rate);
    info.set_channels(CHANNELS as u32);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let format: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map_err(|e| anyhow!("serializing audio format: {e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&format).ok_or_else(|| anyhow!("invalid format pod"))?];

    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    let _ = ready.send(Ok(()));
    mainloop.run();
    Ok(())
}
