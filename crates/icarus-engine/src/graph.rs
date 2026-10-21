use crate::devices::{self, Block, Ctx, Processor};
use crate::{Curve, ParamSpec, fnv1a};
use icarus_model::{OUTPUT_NODE, Project};
use std::collections::{BTreeMap, HashMap};

/// Largest number of frames [`Graph::process`] accepts in one call.
pub const MAX_BLOCK: usize = 1024;

/// Time constant for smoothing base-value changes, so a jump never clicks.
const SMOOTHING_S: f32 = 0.005;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileError {
    #[error("node `{node}` has unknown device kind `{kind}`")]
    UnknownKind { node: String, kind: String },
    #[error("node `{node}` ({kind}) has no parameter `{param}`")]
    UnknownParam { node: String, kind: String, param: String },
    #[error("connection refers to missing node `{0}`")]
    MissingNode(String),
    #[error("the project has no `{OUTPUT_NODE}` output node")]
    NoOutput,
    #[error("the routing contains a cycle")]
    Cycle,
}

/// Address of one parameter inside a compiled graph, for realtime updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamAddr {
    pub node: usize,
    pub param: usize,
}

struct ParamRt {
    spec: &'static ParamSpec,
    base: f32,
    current: f32,
    /// Automation as (sample position, value), sorted by position.
    lane: Vec<(u64, f32)>,
}

impl ParamRt {
    /// Value at an absolute sample position. Pure function of `pos`, which keeps renders
    /// independent of block size.
    fn target(&self, pos: u64) -> f32 {
        let Some(first) = self.lane.first() else {
            return self.base;
        };
        let i = self.lane.partition_point(|(t, _)| *t <= pos);
        if i == 0 {
            return first.1;
        }
        let (t0, v0) = self.lane[i - 1];
        match self.lane.get(i) {
            None => v0,
            Some(&(t1, v1)) => v0 + (v1 - v0) * ((pos - t0) as f32 / (t1 - t0) as f32),
        }
    }
}

struct NodeRt {
    id: String,
    processor: Box<dyn Processor>,
    inputs: Vec<usize>,
    params: Vec<ParamRt>,
    param_bufs: Vec<Vec<f32>>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
}

/// A project compiled into a flat, allocation-free processing schedule.
pub struct Graph {
    ctx: Ctx,
    smoothing: f32,
    nodes: Vec<NodeRt>,
    order: Vec<usize>,
    output: usize,
    index: HashMap<String, usize>,
    length: u64,
    sum_l: Vec<f32>,
    sum_r: Vec<f32>,
}

impl Graph {
    pub fn compile(project: &Project) -> Result<Self, CompileError> {
        let sr = project.sample_rate as f32;
        let index: HashMap<String, usize> = project
            .nodes
            .keys()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        let output = *index.get(OUTPUT_NODE).ok_or(CompileError::NoOutput)?;

        let mut inputs: Vec<Vec<usize>> = vec![Vec::new(); index.len()];
        for c in &project.connections {
            let from = *index
                .get(&c.from)
                .ok_or_else(|| CompileError::MissingNode(c.from.clone()))?;
            let to = *index
                .get(&c.to)
                .ok_or_else(|| CompileError::MissingNode(c.to.clone()))?;
            inputs[to].push(from);
        }

        let mut nodes = Vec::with_capacity(index.len());
        for ((id, node), inputs) in project.nodes.iter().zip(inputs) {
            let unknown_kind = || CompileError::UnknownKind {
                node: id.clone(),
                kind: node.kind.clone(),
            };
            let info = devices::info(&node.kind).ok_or_else(unknown_kind)?;
            let processor =
                devices::create(&node.kind, project.seed ^ fnv1a(id.as_bytes())).ok_or_else(unknown_kind)?;
            for param in node.params.keys().chain(node.automation.keys()) {
                if !info.params.iter().any(|p| p.id == param) {
                    return Err(CompileError::UnknownParam {
                        node: id.clone(),
                        kind: node.kind.clone(),
                        param: param.clone(),
                    });
                }
            }
            let params: Vec<ParamRt> = info
                .params
                .iter()
                .map(|spec| {
                    let base = spec.clamp(node.params.get(spec.id).copied().unwrap_or(spec.default)) as f32;
                    let mut lane: Vec<(u64, f32)> = node
                        .automation
                        .get(spec.id)
                        .into_iter()
                        .flatten()
                        .map(|p| {
                            (
                                (p.time_s.max(0.0) * f64::from(sr)).round() as u64,
                                spec.clamp(p.value) as f32,
                            )
                        })
                        .collect();
                    lane.sort_by_key(|(t, _)| *t);
                    let mut rt = ParamRt {
                        spec,
                        base,
                        current: base,
                        lane,
                    };
                    rt.current = rt.target(0);
                    rt
                })
                .collect();
            nodes.push(NodeRt {
                id: id.clone(),
                processor,
                inputs,
                param_bufs: vec![vec![0.0; MAX_BLOCK]; params.len()],
                params,
                out_l: vec![0.0; MAX_BLOCK],
                out_r: vec![0.0; MAX_BLOCK],
            });
        }

        Ok(Self {
            ctx: Ctx { sample_rate: sr },
            smoothing: 1.0 - (-1.0 / (SMOOTHING_S * sr)).exp(),
            order: topo_order(&nodes)?,
            nodes,
            output,
            index,
            length: (project.length_s.max(0.0) * f64::from(sr)).round().max(1.0) as u64,
            sum_l: vec![0.0; MAX_BLOCK],
            sum_r: vec![0.0; MAX_BLOCK],
        })
    }

    /// Processes `frames` frames starting at absolute sample position `pos`.
    pub fn process(&mut self, pos: u64, frames: usize) {
        assert!(frames <= MAX_BLOCK, "block of {frames} frames exceeds MAX_BLOCK");
        for &n in &self.order {
            self.sum_l[..frames].fill(0.0);
            self.sum_r[..frames].fill(0.0);
            for &src in &self.nodes[n].inputs {
                let src = &self.nodes[src];
                for i in 0..frames {
                    self.sum_l[i] += src.out_l[i];
                    self.sum_r[i] += src.out_r[i];
                }
            }

            let node = &mut self.nodes[n];
            for (p, buf) in node.params.iter_mut().zip(&mut node.param_bufs) {
                for (i, slot) in buf[..frames].iter_mut().enumerate() {
                    let target = p.target(pos + i as u64);
                    p.current = if p.spec.curve == Curve::Stepped {
                        target
                    } else {
                        p.current + self.smoothing * (target - p.current)
                    };
                    *slot = p.current;
                }
            }
            node.processor.process(
                &self.ctx,
                Block {
                    frames,
                    params: &node.param_bufs,
                    in_l: &self.sum_l,
                    in_r: &self.sum_r,
                    out_l: &mut node.out_l,
                    out_r: &mut node.out_r,
                },
            );
        }
    }

    /// Master output of the last processed block.
    pub fn output(&self, frames: usize) -> (&[f32], &[f32]) {
        self.node_output(self.output, frames)
    }

    /// Output of any node for the last processed block: a tap anywhere in the chain.
    pub fn node_output(&self, node: usize, frames: usize) -> (&[f32], &[f32]) {
        let n = &self.nodes[node];
        (&n.out_l[..frames], &n.out_r[..frames])
    }

    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    pub fn param_addr(&self, node: &str, param: &str) -> Option<ParamAddr> {
        let n = self.node_index(node)?;
        let p = self.nodes[n].params.iter().position(|p| p.spec.id == param)?;
        Some(ParamAddr { node: n, param: p })
    }

    /// Sets a base value; it is smoothed in over a few milliseconds. Realtime-safe.
    pub fn set_param(&mut self, addr: ParamAddr, value: f32) {
        let p = &mut self.nodes[addr.node].params[addr.param];
        p.base = p.spec.clamp(f64::from(value)) as f32;
    }

    /// Every parameter's address, keyed by `(node, param)`.
    pub fn param_addrs(&self) -> BTreeMap<(String, String), ParamAddr> {
        self.nodes
            .iter()
            .enumerate()
            .flat_map(|(n, node)| {
                node.params.iter().enumerate().map(move |(p, param)| {
                    (
                        (node.id.clone(), param.spec.id.to_string()),
                        ParamAddr { node: n, param: p },
                    )
                })
            })
            .collect()
    }

    pub fn sample_rate(&self) -> u32 {
        self.ctx.sample_rate as u32
    }

    /// Loop length in samples.
    pub fn length(&self) -> u64 {
        self.length
    }
}

/// Kahn's algorithm. Ties resolve by node index, so the order is the same on every compile.
fn topo_order(nodes: &[NodeRt]) -> Result<Vec<usize>, CompileError> {
    let mut outputs: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut pending: Vec<usize> = nodes.iter().map(|n| n.inputs.len()).collect();
    for (to, node) in nodes.iter().enumerate() {
        for &from in &node.inputs {
            outputs[from].push(to);
        }
    }
    let mut ready: std::collections::BTreeSet<usize> = (0..nodes.len()).filter(|&n| pending[n] == 0).collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(n) = ready.pop_first() {
        order.push(n);
        for &next in &outputs[n] {
            pending[next] -= 1;
            if pending[next] == 0 {
                ready.insert(next);
            }
        }
    }
    if order.len() == nodes.len() {
        Ok(order)
    } else {
        Err(CompileError::Cycle)
    }
}
