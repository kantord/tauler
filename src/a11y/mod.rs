//! Accessibility: the a11y node tree tauler pushes per Panel per Repaint, and the
//! routing of an AT's `Activate` back into the click pipeline.
//!
//! See `docs/adr/0038` (a11y rides the same interaction pipeline as clicks) and
//! `docs/adr/0039` (a11y lives in the root crate). The two decisions that shape
//! everything here:
//!
//! - **The tree is rebuilt whole each Repaint and reconciled by `NodeId`, which is
//!   the `data-tauler-path` child-index path.** Nothing survives between Ticks, the
//!   same way the layout tree doesn't. The `NodeId` for a node is a pure function of
//!   its path, so a node whose path is unchanged keeps its identity across rebuilds
//!   and the platform reconciles it instead of churning it.
//! - **Activate is not a second intent path.** The platform delivers an action on its
//!   own thread, where the QuickJS evaluator cannot be reached (`App::resolve` is the
//!   only place a `$handler` becomes intents, and it lives on the tick thread). So an
//!   activate is reduced to the raw `(panel_id, path)` here and shipped back to the
//!   tick thread, which re-derives the node's `on_click` with the same walk a click
//!   uses and dispatches it through the existing outbox.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use accesskit::{
    Action, ActionHandler, ActivationHandler, DeactivationHandler, Node, NodeId, Rect, Role,
    TreeId, TreeInfo, TreeUpdate,
};
use accesskit_unix::Adapter;

use crate::hit_test::{painted_boxes, Rect as HitRect};

/// Tags whose subtree is dropped from the layout tree entirely.
///
/// Must agree with `crate::layout::html::DROPPED_TAGS`: a dropped tag consumes no
/// sibling slot, so getting this wrong would shift every child-index path that
/// follows it and break the `data-tauler-path` identity this module keys on.
const DROPPED_TAGS: [&str; 5] = ["head", "meta", "link", "style", "script"];

/// Cap on element nesting, guarding this recursive walk against a layout file that
/// nests without bound — the same guard `layout::html::MAX_DEPTH` exists for.
const MAX_DEPTH: usize = 32;

/// The pointer an AT activation fabricates is a press with `x`/`y`/`press_x`/
/// `press_y` of `0` and real width/height (ADR 0038); the button state of a press
/// is `1`.
pub const ACTIVATE_BUTTONS: u16 = 1;

/// Derive the `NodeId` for a child-index path.
///
/// A pure function of the path, which is what lets the platform reconcile the tree
/// by identity across rebuilds (ADR 0038). FNV-1a over a length-prefixed encoding;
/// collision risk is negligible for the handful of nodes a bar carries, and the
/// reverse map kept alongside the tree resolves the path on `Activate` regardless.
fn node_id(path: &[usize]) -> u64 {
    if path.is_empty() {
        return 0;
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in (path.len() as u64).to_le_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for index in path {
        for b in (*index as u64).to_le_bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

/// One element as the a11y tree will present it, gathered during the layout walk.
struct Elem {
    path: Vec<usize>,
    role: Role,
    interactive: bool,
    label: String,
    has_on_click: bool,
}

/// Build the full a11y tree for one panel's contents.
///
/// Runs the same geometry walk a click does (`painted_boxes`) and walks the layout
/// tree to pair every element with its role, name and `on_click`. Returns the
/// `TreeUpdate` plus the `NodeId`-to-path reverse map used to answer an `Activate`.
///
/// A plain `<div on_click>` stays generic — no implicit interactive role (ADR 0038).
/// Only an element with an interactive `role` becomes activatable, and only then when
/// it also carries an `on_click`; an `Activate` on a node with none does nothing.
pub fn build_tree(
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
) -> Option<(TreeUpdate, HashMap<u64, Vec<usize>>)> {
    let geometry: HashMap<Vec<usize>, HitRect> = painted_boxes(content, width, height, dpr)
        .into_iter()
        .collect();
    let mut elems = collect(content)?;
    // A single top-level node has its leading index stripped, the same
    // normalization `layout::html::build_tree` applies to the paths it binds by.
    if count_top_level(content) == 1 {
        for elem in elems.iter_mut() {
            elem.path.remove(0);
        }
    }

    // Build parent -> child order from the paths: the parent of `[a, b]` is `[a]`,
    // and children of one parent are ordered by their trailing sibling index.
    let mut children: HashMap<Vec<usize>, Vec<Vec<usize>>> = HashMap::new();
    for elem in &elems {
        if elem.path.is_empty() {
            continue;
        }
        let (parent, _) = elem.path.split_at(elem.path.len() - 1);
        children
            .entry(parent.to_vec())
            .or_default()
            .push(elem.path.clone());
    }
    for paths in children.values_mut() {
        paths.sort();
    }

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(elems.len());
    let mut reverse: HashMap<u64, Vec<usize>> = HashMap::with_capacity(elems.len());
    let mut root: Option<NodeId> = None;

    for elem in &elems {
        let id = node_id(&elem.path);
        // The panel's root element is its top-level window, so it gets the window
        // role that makes at-spi register it as a window (a toplevel for the AT).
        let is_root = elem.path.is_empty();
        let role = if is_root { Role::Window } else { elem.role };
        let mut node = Node::new(role);
        if !elem.label.is_empty() {
            node.set_label(elem.label.clone());
        }
        let child_ids: Vec<NodeId> = children
            .get(&elem.path)
            .map(|paths| paths.iter().map(|p| NodeId(node_id(p))).collect())
            .unwrap_or_default();
        node.set_children(child_ids);
        if let Some(bounds) = rect_for(&geometry, &elem.path) {
            node.set_bounds(bounds);
        }
        if !is_root && elem.interactive && elem.has_on_click {
            node.add_action(Action::Click);
        }
        if is_root {
            root = Some(NodeId(id));
        }
        reverse.insert(id, elem.path.clone());
        nodes.push((NodeId(id), node));
    }

    let root = root?;
    Some((
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        },
        reverse,
    ))
}

/// How many top-level nodes the root produces, matching `layout::html::build_tree`'s
/// `nodes.len()`: a single element (or scalar) yields one, an array may yield many.
fn count_top_level(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(_) | serde_json::Value::Number(_) => 1,
        serde_json::Value::Bool(_) | serde_json::Value::Null => 0,
        serde_json::Value::Array(items) => items.iter().map(count_top_level).sum(),
        serde_json::Value::Object(_) => {
            let tag = value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if DROPPED_TAGS.contains(&tag) || tag == "br" {
                0
            } else {
                1
            }
        }
    }
}

/// Walk the layout tree, gathering every element with the child-index path a click's
/// binding uses (before the single-root leading-index strip).
fn collect(value: &serde_json::Value) -> Option<Vec<Elem>> {
    let mut elems = Vec::new();
    let mut path = Vec::new();
    process(value, 0, &mut path, &mut elems)?;
    Some(elems)
}

/// `count` is the number of children seen so far in the current sibling list — text
/// nodes count, so an element's index matches the one `layout::html` assigns.
fn process(
    value: &serde_json::Value,
    depth: usize,
    path: &mut Vec<usize>,
    elems: &mut Vec<Elem>,
) -> Option<()> {
    let mut count = 0usize;
    process_value(value, depth, path, &mut count, elems)
}

fn process_value(
    value: &serde_json::Value,
    depth: usize,
    path: &mut Vec<usize>,
    count: &mut usize,
    elems: &mut Vec<Elem>,
) -> Option<()> {
    match value {
        serde_json::Value::String(_) | serde_json::Value::Number(_) => {
            *count += 1;
            Some(())
        }
        serde_json::Value::Bool(_) | serde_json::Value::Null => Some(()),
        serde_json::Value::Array(items) => {
            for item in items {
                process_value(item, depth, path, count, elems)?;
            }
            Some(())
        }
        serde_json::Value::Object(_) => {
            if depth >= MAX_DEPTH {
                return None;
            }
            let tag = value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if tag == "svg" {
                return None;
            }
            let dropped = DROPPED_TAGS.contains(&tag);
            path.push(*count);
            if !dropped && tag != "br" {
                let (role, interactive) = role_of(value, tag);
                elems.push(Elem {
                    path: path.clone(),
                    role,
                    interactive,
                    label: accessible_name(value),
                    has_on_click: value.get("on_click").is_some(),
                });
            }
            if !dropped && tag != "br" {
                if let Some(children) = value.get("children") {
                    process(children, depth + 1, path, elems)?;
                }
            }
            path.pop();
            if !dropped {
                *count += 1;
            }
            Some(())
        }
    }
}

/// The element's role: its explicit `role` attribute, else a default from what it
/// carries. `interactive` is whether an AT may `Activate` it — only ever true for an
/// explicit interactive role, never for a plain element (ADR 0038).
fn role_of(value: &serde_json::Value, tag: &str) -> (Role, bool) {
    if let Some(role) = value.get("role").and_then(serde_json::Value::as_str) {
        return match role {
            "button" => (Role::Button, true),
            "link" => (Role::Link, true),
            "checkbox" => (Role::CheckBox, true),
            "switch" => (Role::Switch, true),
            "slider" => (Role::Slider, true),
            "tab" => (Role::Tab, true),
            "radio" => (Role::RadioButton, true),
            "img" => (Role::Image, false),
            _ => (Role::GenericContainer, false),
        };
    }
    if tag == "img" {
        return (Role::Image, false);
    }
    if !accessible_name(value).is_empty() {
        (Role::Paragraph, false)
    } else {
        (Role::GenericContainer, false)
    }
}

/// A node's box, in physical pixels — the same read-back `hit_test` uses.
fn rect_for(geometry: &HashMap<Vec<usize>, HitRect>, path: &[usize]) -> Option<Rect> {
    geometry.get(path).map(|r| {
        Rect::from_origin_size(
            accesskit::Point {
                x: r.x as f64,
                y: r.y as f64,
            },
            accesskit::Size {
                width: r.width as f64,
                height: r.height as f64,
            },
        )
    })
}

/// The node's accessible name: `aria-label`, then `title`, then its text content.
fn accessible_name(value: &serde_json::Value) -> String {
    if let Some(label) = value.get("aria-label").and_then(serde_json::Value::as_str) {
        return label.to_string();
    }
    if let Some(title) = value.get("title").and_then(serde_json::Value::as_str) {
        return title.to_string();
    }
    let mut text = String::new();
    text_content(value, &mut text);
    text
}

/// All text within an element's subtree, concatenated.
fn text_content(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::Array(items) => {
            for item in items {
                text_content(item, out);
            }
        }
        serde_json::Value::Object(_) => {
            if let Some(children) = value.get("children") {
                text_content(children, out);
            }
        }
        serde_json::Value::Bool(_) | serde_json::Value::Null => {}
    }
}

/// Re-derive the click handler a node path should fire, the same walk `hit_test`
/// does on a press. Returns the `on_click` value and the element's box so the caller
/// can resolve and dispatch with a fabricated press at the box origin.
///
/// This is what `App::resolve`/`App::send` needs to reach the tick thread's pipeline
/// (ADR 0038). `on_click` is whatever the layout declared — an array of intents or a
/// `$handler` id — and `rect` gives the pointer its width/height.
pub fn click_at_path(
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
    path: &[usize],
) -> Option<(serde_json::Value, HitRect)> {
    let (_, bindings) = crate::layout::html::build_tree(content).ok()?;
    let binding = bindings.iter().find(|b| b.path == path)?;
    let on_click = binding.on_click.clone()?;
    let rect = painted_boxes(content, width, height, dpr)
        .into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, r)| r)
        .unwrap_or(HitRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
    Some((on_click, rect))
}

/// The latest content + geometry one panel exposes to the a11y thread.
struct PanelState {
    content: serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
    /// `NodeId` -> child-index path, refreshed whenever the tree is rebuilt.
    reverse: HashMap<u64, Vec<usize>>,
}

type SharedState = Arc<Mutex<Option<PanelState>>>;

struct PanelActivation {
    state: SharedState,
}

impl ActivationHandler for PanelActivation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let (content, width, height, dpr) = {
            let guard = self.state.lock().ok()?;
            let s = guard.as_ref()?;
            (s.content.clone(), s.width, s.height, s.dpr)
        };
        let (update, reverse) = build_tree(&content, width, height, dpr)?;
        if let Ok(mut s) = self.state.lock() {
            if let Some(s) = s.as_mut() {
                s.reverse = reverse;
            }
        }
        Some(update)
    }
}

struct PanelActions {
    panel_id: String,
    state: SharedState,
    action_tx: mpsc::Sender<(String, Vec<usize>)>,
    /// Pings the data loop so the tick thread wakes to drain the action.
    notifier: mpsc::SyncSender<()>,
}

impl ActionHandler for PanelActions {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        if request.action != Action::Click {
            return;
        }
        let Ok(guard) = self.state.lock() else { return };
        let Some(path) = guard
            .as_ref()
            .and_then(|s| s.reverse.get(&request.target_node.0).cloned())
        else {
            return;
        };
        drop(guard);
        if self.action_tx.send((self.panel_id.clone(), path)).is_ok() {
            // The tick thread is asleep on the data loop's notifier; an
            // activation arriving on accesskit's thread must wake it or the
            // intent sits in the channel until the next natural pass.
            let _ = self.notifier.try_send(());
        }
    }
}

struct PanelDeactivation;

impl DeactivationHandler for PanelDeactivation {
    fn deactivate_accessibility(&mut self) {}
}

struct PanelAdapter {
    adapter: Adapter,
    state: SharedState,
}

/// What the tick thread wants each panel's a11y tree to be.
pub struct PanelInfo {
    pub id: String,
    pub content: serde_json::Value,
    pub width: u32,
    pub height: u32,
    pub dpr: f32,
}

/// Manages the per-panel a11y adapters, pushing each panel's tree on content change.
///
/// One adapter per panel: each panel is a desktop window, so each is a top-level in
/// the platform tree. `update_if_active` keeps an unattached tree ~free — the tree
/// is only built, and only pushed, once an AT is actually attached (ADR 0039).
pub struct A11y {
    adapters: HashMap<String, PanelAdapter>,
    action_tx: mpsc::Sender<(String, Vec<usize>)>,
    notifier: mpsc::SyncSender<()>,
}

impl A11y {
    pub fn new(notifier: mpsc::SyncSender<()>) -> (Self, mpsc::Receiver<(String, Vec<usize>)>) {
        let (action_tx, action_rx) = mpsc::channel();
        (
            Self {
                adapters: HashMap::new(),
                action_tx,
                notifier,
            },
            action_rx,
        )
    }

    /// Reconcile the a11y trees to the current panel set.
    pub fn reconcile(&mut self, panels: &[PanelInfo]) {
        let desired: std::collections::HashSet<&str> =
            panels.iter().map(|p| p.id.as_str()).collect();
        self.adapters.retain(|id, _| desired.contains(id.as_str()));

        for panel in panels {
            let entry = self.adapters.entry(panel.id.clone()).or_insert_with(|| {
                create_panel_adapter(
                    panel.id.clone(),
                    self.action_tx.clone(),
                    self.notifier.clone(),
                )
            });
            let changed = {
                let mut guard = entry.state.lock().unwrap();
                match guard.as_mut() {
                    Some(s)
                        if s.content == panel.content
                            && s.width == panel.width
                            && s.height == panel.height
                            && s.dpr == panel.dpr =>
                    {
                        false
                    }
                    _ => {
                        let reverse = guard
                            .as_ref()
                            .map(|s| s.reverse.clone())
                            .unwrap_or_default();
                        *guard = Some(PanelState {
                            content: panel.content.clone(),
                            width: panel.width,
                            height: panel.height,
                            dpr: panel.dpr,
                            reverse,
                        });
                        true
                    }
                }
            };
            if changed {
                let state = Arc::clone(&entry.state);
                entry.adapter.update_if_active(move || {
                    let mut guard = state.lock().unwrap();
                    match guard.as_ref() {
                        Some(s) => match build_tree(&s.content, s.width, s.height, s.dpr) {
                            Some((update, reverse)) => {
                                if let Some(inner) = guard.as_mut() {
                                    inner.reverse = reverse;
                                }
                                update
                            }
                            None => empty_tree(),
                        },
                        None => empty_tree(),
                    }
                });
            }
        }
    }
}

fn create_panel_adapter(
    panel_id: String,
    action_tx: mpsc::Sender<(String, Vec<usize>)>,
    notifier: mpsc::SyncSender<()>,
) -> PanelAdapter {
    let state: SharedState = Arc::new(Mutex::new(None));
    let adapter = Adapter::new(
        PanelActivation {
            state: Arc::clone(&state),
        },
        PanelActions {
            panel_id,
            state: Arc::clone(&state),
            action_tx,
            notifier,
        },
        PanelDeactivation,
    );
    PanelAdapter { adapter, state }
}

fn empty_tree() -> TreeUpdate {
    let root = NodeId(0);
    let node = Node::new(Role::GenericContainer);
    TreeUpdate {
        nodes: vec![(root, node)],
        tree: Some(TreeInfo::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> serde_json::Value {
        serde_json::json!({
            "type": "div",
            "style": {"width": 200, "height": 100},
            "children": [
                { "type": "div", "class": "vol", "children": ["50%"] },
                {
                    "type": "div",
                    "role": "button",
                    "aria-label": "Mute",
                    "on_click": [{"channel": "t", "event": {"type": "mute"}}],
                    "children": ["M"]
                }
            ]
        })
    }

    fn tree() -> (TreeUpdate, HashMap<u64, Vec<usize>>) {
        init_ctx();
        build_tree(&layout(), 200, 100, 1.0).expect("a tree builds")
    }

    fn init_ctx() {
        use crate::config::FontConfig;
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| crate::init_global_ctx(FontConfig::default()));
    }

    fn find<'a>(update: &'a TreeUpdate, path: &[usize]) -> &'a Node {
        let id = node_id(path);
        update
            .nodes
            .iter()
            .find(|(n, _)| n.0 == id)
            .map(|(_, n)| n)
            .expect("node exists")
    }

    /// The point of the whole module: a node the author made a button is activatable,
    /// and it is identified by the same child-index path a click binds by.
    #[test]
    fn a_button_node_is_activatable_at_its_path() {
        let (update, reverse) = tree();
        let path = vec![1];
        let node = find(&update, &path);
        assert_eq!(node.role(), Role::Button);
        assert!(node.supports_action(Action::Click));
        assert_eq!(node.label(), Some("Mute"));
        assert!(node.bounds().is_some());
        assert_eq!(reverse.get(&node_id(&path)), Some(&path));
    }

    /// A plain `<div on_click>` gets no implicit interactive role, so an AT cannot
    /// activate it — the consequence ADR 0038 calls out explicitly.
    #[test]
    fn a_plain_div_is_not_activatable_even_with_on_click() {
        let layout = serde_json::json!({
            "type": "div",
            "children": [
                { "type": "div", "on_click": [{"channel": "t", "event": {"type": "x"}}], "children": ["hi"] }
            ]
        });
        init_ctx();
        let (update, _) = build_tree(&layout, 200, 100, 1.0).unwrap();
        let node = find(&update, &[0]);
        assert!(!node.supports_action(Action::Click));
        assert_ne!(node.role(), Role::Button);
    }

    /// The NodeId is a pure function of the path, which is what makes platform
    /// reconciliation by identity possible across rebuilds.
    #[test]
    fn node_id_is_deterministic_and_distinct() {
        let a = node_id(&[0, 1, 2]);
        let b = node_id(&[0, 1, 2]);
        assert_eq!(a, b);
        assert_ne!(a, node_id(&[0, 1, 3]));
        assert_ne!(node_id(&[1, 2]), node_id(&[1, 2, 0]), "length must matter");
    }

    /// Activation re-derives the same on_click a click would, with a real box.
    #[test]
    fn click_at_path_returns_the_on_click_and_a_box() {
        init_ctx();
        let (on_click, rect) =
            click_at_path(&layout(), 200, 100, 1.0, &[1]).expect("handler found");
        assert_eq!(on_click[0]["event"], serde_json::json!({"type": "mute"}));
        assert!(rect.width > 0.0 && rect.height > 0.0);
    }

    /// A node with no on_click yields no handler — an Activate there does nothing,
    /// exactly as a click on one would.
    #[test]
    fn click_at_path_for_a_non_interactive_node_is_none() {
        init_ctx();
        assert!(click_at_path(&layout(), 200, 100, 1.0, &[0]).is_none());
    }
}
