//! Change events and subscriptions.
//!
//! Every change to a session becomes an [`Event`]: which client changed which paths to what.
//! Changes are found by diffing the path tree before and after, so any state that has a path
//! is covered automatically. Clients open a feed and subscribe it to path prefixes.

use icarus_engine::devices;
use icarus_model::Project;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{Receiver, Sender, channel};

pub type FeedId = u64;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Change {
    pub path: String,
    /// New value; `null` when the path no longer exists.
    pub value: Value,
    /// For parameters, the value on the control's 0..1 range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Event {
    /// Session-wide sequence number. A gap means a feed missed events and should re-read state.
    pub seq: u64,
    pub client: String,
    pub label: String,
    pub changes: Vec<Change>,
}

struct Feed {
    tx: Sender<Event>,
    /// Normalized prefixes; `""` matches everything.
    paths: BTreeSet<String>,
}

#[derive(Default)]
pub(crate) struct Feeds {
    next: FeedId,
    seq: u64,
    feeds: BTreeMap<FeedId, Feed>,
}

/// `"nodes/filt/"` → `"/nodes/filt"`; `"/"` → `""`.
pub(crate) fn normalize(path: &str) -> String {
    path.split('/')
        .filter(|s| !s.is_empty())
        .fold(String::new(), |acc, s| acc + "/" + s)
}

fn matches(subscription: &str, path: &str) -> bool {
    subscription.is_empty()
        || path == subscription
        || path.strip_prefix(subscription).is_some_and(|rest| rest.starts_with('/'))
        // Subscribing to a param's /normalized view still delivers the param.
        || subscription.strip_suffix("/normalized") == Some(path)
}

impl Feeds {
    pub fn open(&mut self) -> (FeedId, Receiver<Event>) {
        let (tx, rx) = channel();
        self.next += 1;
        self.feeds.insert(
            self.next,
            Feed {
                tx,
                paths: BTreeSet::new(),
            },
        );
        (self.next, rx)
    }

    pub fn close(&mut self, id: FeedId) {
        self.feeds.remove(&id);
    }

    pub fn subscribe(&mut self, id: FeedId, paths: &[String]) -> Option<Vec<String>> {
        let feed = self.feeds.get_mut(&id)?;
        feed.paths.extend(paths.iter().map(|p| normalize(p)));
        Some(
            feed.paths
                .iter()
                .map(|p| if p.is_empty() { "/".into() } else { p.clone() })
                .collect(),
        )
    }

    /// Removes the given prefixes, or all of them when `paths` is empty.
    pub fn unsubscribe(&mut self, id: FeedId, paths: &[String]) -> Option<Vec<String>> {
        let feed = self.feeds.get_mut(&id)?;
        if paths.is_empty() {
            feed.paths.clear();
        }
        for p in paths {
            feed.paths.remove(&normalize(p));
        }
        Some(feed.paths.iter().cloned().collect())
    }

    /// Sends each feed the changes it subscribed to. Feeds whose receiver is gone are dropped.
    pub fn emit(&mut self, client: &str, label: &str, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        self.feeds.retain(|_, feed| {
            let wanted: Vec<Change> = changes
                .iter()
                .filter(|c| feed.paths.iter().any(|s| matches(s, &c.path)))
                .cloned()
                .collect();
            wanted.is_empty()
                || feed
                    .tx
                    .send(Event {
                        seq,
                        client: client.to_string(),
                        label: label.to_string(),
                        changes: wanted,
                    })
                    .is_ok()
        });
    }
}

/// The value at a param path plus its normalized position.
pub(crate) fn param_change(project: &Project, node: &str, param: &str) -> Option<Change> {
    let n = project.nodes.get(node)?;
    let spec = devices::info(&n.kind)?.params.iter().find(|p| p.id == param)?;
    let value = n.params.get(param).copied().unwrap_or(spec.default);
    Some(Change {
        path: format!("/nodes/{node}/params/{param}"),
        value: json!(value),
        normalized: Some(spec.to_normalized(value)),
    })
}

/// Every leaf path of the project with its value.
fn flatten(project: &Project) -> BTreeMap<String, Change> {
    let mut leaves = BTreeMap::new();
    let mut params = Vec::new();
    let mut put = |path: String, value: Value| {
        leaves.insert(
            path.clone(),
            Change {
                path,
                value,
                normalized: None,
            },
        );
    };
    put("/project/sample_rate".into(), json!(project.sample_rate));
    put("/project/seed".into(), json!(project.seed));
    put("/project/tempo_bpm".into(), json!(project.tempo_bpm));
    put("/project/length_s".into(), json!(project.length_s));
    put("/connections".into(), json!(project.connections));
    for (id, node) in &project.nodes {
        put(format!("/nodes/{id}/kind"), json!(node.kind));
        for (param, points) in &node.automation {
            put(format!("/nodes/{id}/automation/{param}"), json!(points));
        }
        for spec in devices::info(&node.kind).map(|d| d.params).unwrap_or_default() {
            params.push(param_change(project, id, spec.id).expect("spec exists"));
        }
    }
    leaves.extend(params.into_iter().map(|c| (c.path.clone(), c)));
    flatten_json("/ui", &project.ui, &mut leaves);
    leaves
}

/// Non-empty objects are walked into; everything else (arrays, strings, numbers) is one leaf.
fn flatten_json(prefix: &str, value: &Value, leaves: &mut BTreeMap<String, Change>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, v) in map {
                flatten_json(&format!("{prefix}/{key}"), v, leaves);
            }
        }
        _ => {
            leaves.insert(
                prefix.to_string(),
                Change {
                    path: prefix.to_string(),
                    value: value.clone(),
                    normalized: None,
                },
            );
        }
    }
}

/// Paths whose values differ between two projects; removed paths become `null`.
/// Removals come first, so a client applying changes in order never deletes something it was
/// just given (e.g. when a leaf becomes an object).
pub(crate) fn diff(before: &Project, after: &Project) -> Vec<Change> {
    let old = flatten(before);
    let new = flatten(after);
    let mut changes: Vec<Change> = old
        .keys()
        .filter(|p| !new.contains_key(*p))
        .map(|path| Change {
            path: path.clone(),
            value: Value::Null,
            normalized: None,
        })
        .collect();
    changes.extend(
        new.iter()
            .filter(|(path, c)| old.get(*path).map(|o| &o.value) != Some(&c.value))
            .map(|(_, c)| c.clone()),
    );
    changes
}
