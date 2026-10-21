//! Offline forked rendering.
//!
//! A render takes a copy of the project and runs the engine without an audio device, as fast
//! as the CPU allows. Because the engine is deterministic, renders of forked copies that differ
//! in one parameter differ only because of that parameter, which is what makes probing work.

use icarus_analysis::{Features, analyze};
use icarus_engine::{CompileError, Graph, MAX_BLOCK, devices};
use icarus_model::{OUTPUT_NODE, ProbeSpec, Project, RenderSpec};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;

/// Upper bound on the number of variants one probe may render.
pub const MAX_PROBE_VARIANTS: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error("invalid region {start_s}..{end_s} s")]
    BadRegion { start_s: f64, end_s: f64 },
    #[error("no node `{0}` to tap")]
    UnknownTap(String),
    #[error("node `{node}` has no parameter `{param}`")]
    UnknownParam { node: String, param: String },
    #[error("probe would render {0} variants (limit {MAX_PROBE_VARIANTS})")]
    TooManyVariants(usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

pub struct Rendered {
    pub sample_rate: u32,
    pub master: Stereo,
    pub taps: BTreeMap<String, Stereo>,
}

/// Renders `spec`'s region. Processing always starts at sample 0 so that filter and
/// oscillator state at the start of the region is the same as during playback.
pub fn render(project: &Project, spec: &RenderSpec) -> Result<Rendered, RenderError> {
    if !(spec.start_s >= 0.0 && spec.end_s > spec.start_s) {
        return Err(RenderError::BadRegion {
            start_s: spec.start_s,
            end_s: spec.end_s,
        });
    }
    let mut graph = Graph::compile(project)?;
    let sr = f64::from(project.sample_rate);
    let start = (spec.start_s * sr).round() as u64;
    let end = (spec.end_s * sr).round() as u64;
    let block = spec.block_size.clamp(1, MAX_BLOCK);

    let taps: Vec<(String, usize)> = spec
        .taps
        .iter()
        .map(|t| {
            graph
                .node_index(t)
                .map(|i| (t.clone(), i))
                .ok_or_else(|| RenderError::UnknownTap(t.clone()))
        })
        .collect::<Result<_, _>>()?;
    let mut master = Stereo::default();
    let mut tapped: BTreeMap<String, Stereo> = taps.iter().map(|(id, _)| (id.clone(), Stereo::default())).collect();

    let mut pos = 0u64;
    while pos < end {
        let n = block.min((end - pos) as usize);
        graph.process(pos, n);
        // Keep only the frames inside the region.
        let skip = start.saturating_sub(pos).min(n as u64) as usize;
        let keep = |dst: &mut Stereo, (l, r): (&[f32], &[f32])| {
            dst.l.extend_from_slice(&l[skip..]);
            dst.r.extend_from_slice(&r[skip..]);
        };
        keep(&mut master, graph.output(n));
        for (id, idx) in &taps {
            keep(
                tapped.get_mut(id).expect("tap buffer exists"),
                graph.node_output(*idx, n),
            );
        }
        pos += n as u64;
    }

    Ok(Rendered {
        sample_rate: project.sample_rate,
        master,
        taps: tapped,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeRow {
    /// `"node.param"` to the value used in this variant.
    pub settings: BTreeMap<String, f64>,
    pub features: Features,
}

/// How much one feature moves across one parameter's full (normalized 0..1) range.
#[derive(Clone, Debug, Serialize)]
pub struct Sensitivity {
    pub param: String,
    pub feature: &'static str,
    /// Least-squares slope of the feature against the normalized parameter value.
    pub change_over_full_range: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeResult {
    pub listen: String,
    pub rows: Vec<ProbeRow>,
    /// Sorted with the strongest effects first.
    pub sensitivity: Vec<Sensitivity>,
}

/// Renders every combination of the sweeps in parallel, each on its own fork of the project.
/// A swept parameter's automation lane is removed in the fork so the swept value takes effect.
pub fn probe(project: &Project, spec: &ProbeSpec) -> Result<ProbeResult, RenderError> {
    let listen = spec.listen.clone().unwrap_or_else(|| OUTPUT_NODE.to_string());
    let mut param_specs = Vec::with_capacity(spec.sweeps.len());
    for s in &spec.sweeps {
        let unknown = || RenderError::UnknownParam {
            node: s.node.clone(),
            param: s.param.clone(),
        };
        let node = project.nodes.get(&s.node).ok_or_else(unknown)?;
        let info = devices::info(&node.kind).ok_or_else(unknown)?;
        param_specs.push(*info.params.iter().find(|p| p.id == s.param).ok_or_else(unknown)?);
    }

    let variants = spec.sweeps.iter().map(|s| s.values.len()).product::<usize>();
    if variants > MAX_PROBE_VARIANTS {
        return Err(RenderError::TooManyVariants(variants));
    }

    // Each variant is a list of indices into its sweep's values (row-major).
    let combos: Vec<Vec<usize>> = (0..variants)
        .map(|mut k| {
            let mut combo = vec![0; spec.sweeps.len()];
            for (slot, s) in combo.iter_mut().zip(&spec.sweeps).rev() {
                *slot = k % s.values.len();
                k /= s.values.len();
            }
            combo
        })
        .collect();

    let render_spec = RenderSpec {
        start_s: spec.start_s,
        end_s: spec.end_s,
        block_size: MAX_BLOCK,
        taps: vec![listen.clone()],
    };
    let rows = combos
        .par_iter()
        .map(|combo| {
            let mut fork = project.clone();
            let mut settings = BTreeMap::new();
            for ((s, &i), pspec) in spec.sweeps.iter().zip(combo).zip(&param_specs) {
                let value = pspec.clamp(s.values[i]);
                let node = fork.nodes.get_mut(&s.node).expect("validated above");
                node.params.insert(s.param.clone(), value);
                node.automation.remove(&s.param);
                settings.insert(format!("{}.{}", s.node, s.param), value);
            }
            let out = render(&fork, &render_spec)?;
            let tap = &out.taps[&listen];
            Ok(ProbeRow {
                settings,
                features: analyze(&tap.l, &tap.r, out.sample_rate),
            })
        })
        .collect::<Result<Vec<_>, RenderError>>()?;

    let mut sensitivity = Vec::new();
    for (s, pspec) in spec.sweeps.iter().zip(&param_specs) {
        let key = format!("{}.{}", s.node, s.param);
        let xs: Vec<f64> = rows.iter().map(|r| pspec.to_normalized(r.settings[&key])).collect();
        for (f, feature) in Features::NAMES.iter().enumerate() {
            let ys: Vec<f64> = rows.iter().map(|r| r.features.values()[f]).collect();
            if let Some(slope) = slope(&xs, &ys) {
                sensitivity.push(Sensitivity {
                    param: key.clone(),
                    feature,
                    change_over_full_range: slope,
                });
            }
        }
    }
    sensitivity.sort_by(|a, b| {
        b.change_over_full_range
            .abs()
            .total_cmp(&a.change_over_full_range.abs())
    });

    Ok(ProbeResult {
        listen,
        rows,
        sensitivity,
    })
}

fn slope(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    if sxx < 1e-12 {
        return None;
    }
    Some(xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / sxx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use icarus_model::{AutoPoint, Connection, Node, Sweep};

    fn chain() -> Project {
        let mut p = Project {
            seed: 7,
            ..Project::default()
        };
        let mut osc = Node::new("osc");
        osc.params.insert("freq".into(), 110.0);
        let mut noise = Node::new("noise");
        noise.params.insert("level".into(), 0.1);
        let mut filt = Node::new("filter");
        filt.automation.insert(
            "cutoff".into(),
            vec![
                AutoPoint {
                    time_s: 0.0,
                    value: 200.0,
                },
                AutoPoint {
                    time_s: 0.5,
                    value: 8_000.0,
                },
            ],
        );
        p.nodes.insert("osc".into(), osc);
        p.nodes.insert("noise".into(), noise);
        p.nodes.insert("filt".into(), filt);
        for (from, to) in [("osc", "filt"), ("noise", "filt"), ("filt", "master")] {
            p.connections.push(Connection {
                from: from.into(),
                to: to.into(),
            });
        }
        p
    }

    fn spec(block_size: usize) -> RenderSpec {
        RenderSpec {
            start_s: 0.1,
            end_s: 0.6,
            block_size,
            taps: vec!["osc".into()],
        }
    }

    #[test]
    fn renders_are_bit_identical() {
        let p = chain();
        let a = render(&p, &spec(256)).unwrap();
        let b = render(&p, &spec(256)).unwrap();
        assert_eq!(a.master, b.master);
        assert_eq!(a.taps, b.taps);
    }

    #[test]
    fn block_size_does_not_change_output() {
        let p = chain();
        let reference = render(&p, &spec(MAX_BLOCK)).unwrap().master;
        for block in [1, 37, 64, 333] {
            assert_eq!(render(&p, &spec(block)).unwrap().master, reference, "block {block}");
        }
    }

    #[test]
    fn region_matches_full_render() {
        let p = chain();
        let full = render(
            &p,
            &RenderSpec {
                start_s: 0.0,
                end_s: 0.6,
                block_size: 128,
                taps: vec![],
            },
        )
        .unwrap();
        let part = render(&p, &spec(128)).unwrap();
        assert_eq!(&full.master.l[4_800..], &part.master.l[..]);
    }

    #[test]
    fn different_seed_changes_noise() {
        let mut p = chain();
        let a = render(&p, &spec(256)).unwrap().master;
        p.seed = 8;
        assert_ne!(render(&p, &spec(256)).unwrap().master, a);
    }

    #[test]
    fn probe_finds_cutoff_brightens() {
        let p = chain();
        let result = probe(
            &p,
            &ProbeSpec {
                start_s: 0.0,
                end_s: 0.5,
                listen: Some("filt".into()),
                sweeps: vec![
                    Sweep {
                        node: "filt".into(),
                        param: "cutoff".into(),
                        values: vec![200.0, 1_000.0, 5_000.0],
                    },
                    Sweep {
                        node: "filt".into(),
                        param: "resonance".into(),
                        values: vec![0.0, 0.5],
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(result.rows.len(), 6);
        let s = result
            .sensitivity
            .iter()
            .find(|s| s.param == "filt.cutoff" && s.feature == "centroid_hz")
            .unwrap();
        assert!(s.change_over_full_range > 100.0, "{s:?}");

        // A probe row must equal a direct render of the same fork.
        let mut fork = p.clone();
        let filt = fork.nodes.get_mut("filt").unwrap();
        filt.automation.clear();
        filt.params.insert("cutoff".into(), 200.0);
        filt.params.insert("resonance".into(), 0.0);
        let direct = render(
            &fork,
            &RenderSpec {
                start_s: 0.0,
                end_s: 0.5,
                block_size: 64,
                taps: vec!["filt".into()],
            },
        )
        .unwrap();
        let tap = &direct.taps["filt"];
        assert_eq!(result.rows[0].features, analyze(&tap.l, &tap.r, 48_000));
    }
}
