//! The PipeWire graph the shell reads:
//! nodes with their volume and mute, links, and the `default` metadata
//! object's four default names. The model here is pure; the `pipewire`
//! feature adds the client thread that feeds it ([`spawn`]).
//!
//! Volumes are the visual scale: the cube root of the linear `channelVolumes` the server
//! holds, the same scale `wpctl get-volume` prints.

use std::collections::BTreeMap;

#[cfg(feature = "pipewire")]
mod backend;
#[cfg(feature = "pipewire")]
pub use backend::{Audio, spawn};

pub const KEY_DEFAULT_SINK: &str = "default.audio.sink";
pub const KEY_DEFAULT_SOURCE: &str = "default.audio.source";
pub const KEY_CONFIGURED_SINK: &str = "default.configured.audio.sink";
pub const KEY_CONFIGURED_SOURCE: &str = "default.configured.audio.source";
pub const DEFAULT_TYPE: &str = "Spa:String:JSON";

/// Linear `channelVolumes` value to the visual scale.
pub fn visual_from_linear(linear: f32) -> f32 {
    linear.cbrt()
}

/// Visual scale to the linear value written back to the server.
pub fn linear_from_visual(visual: f32) -> f32 {
    visual * visual * visual
}

/// The volume setter: every channel scaled by the
/// same factor so their balance holds, or all set flat when the average was
/// zero. Negative results clamp to zero, as `setVolumes` does.
pub fn scaled_volumes(current: &[f32], average: f32) -> Vec<f32> {
    let old = average_volume(current);
    let mul = if old == 0.0 { 0.0 } else { average / old };
    current
        .iter()
        .map(|v| if mul == 0.0 { average } else { v * mul })
        .map(|v| v.max(0.0))
        .collect()
}

pub fn average_volume(volumes: &[f32]) -> f32 {
    if volumes.is_empty() {
        return 0.0;
    }
    volumes.iter().sum::<f32>() / volumes.len() as f32
}

/// `PwNodeType`'s flags, read off `media.class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeType {
    pub audio: bool,
    pub video: bool,
    pub stream: bool,
    pub source: bool,
    pub sink: bool,
}

impl NodeType {
    pub fn from_media_class(class: &str) -> Self {
        let (audio, video, stream, source, sink) = match class {
            "Audio/Sink" => (true, false, false, false, true),
            "Audio/Source" => (true, false, false, true, false),
            "Audio/Duplex" => (true, false, false, true, true),
            "Stream/Output/Audio" => (true, false, true, false, true),
            "Stream/Input/Audio" => (true, false, true, true, false),
            "Video/Sink" => (false, true, false, false, true),
            "Video/Source" => (false, true, false, true, false),
            _ => (false, false, false, false, false),
        };
        Self {
            audio,
            video,
            stream,
            source,
            sink,
        }
    }

    /// `PwNodeType::toString`.
    pub fn name(self) -> &'static str {
        match (self.audio, self.video, self.stream, self.source, self.sink) {
            (true, _, false, false, true) => "AudioSink",
            (true, _, false, true, false) => "AudioSource",
            (true, _, false, true, true) => "AudioDuplex",
            (true, _, true, false, true) => "AudioOutStream",
            (true, _, true, true, false) => "AudioInStream",
            (_, true, _, false, true) => "VideoSink",
            (_, true, _, true, false) => "VideoSource",
            _ => "Untracked",
        }
    }
}

/// Volume and mute, present on every audio node once its params arrive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioState {
    /// `spa_audio_channel` positions, one per entry of `volumes`.
    pub channels: Vec<u32>,
    /// Visual scale, per channel.
    pub volumes: Vec<f32>,
    pub muted: bool,
}

impl AudioState {
    /// The averaged `volume`.
    pub fn volume(&self) -> f32 {
        average_volume(&self.volumes)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    pub id: u32,
    pub serial: Option<u64>,
    pub name: String,
    pub description: String,
    pub nick: String,
    pub media_class: String,
    pub node_type: NodeType,
    /// `media.category` Monitor or Manager.
    pub is_monitor: bool,
    /// The whole property bag, global props overlaid by the bound node's.
    pub props: BTreeMap<String, String>,
    /// `None` for anything that is not an audio node.
    pub audio: Option<AudioState>,
    /// The bound node's first info has arrived, so `props` is complete.
    pub ready: bool,
}

impl Node {
    pub fn from_props(id: u32, props: BTreeMap<String, String>) -> Self {
        let mut node = Self {
            id,
            ..Self::default()
        };
        node.set_props(props);
        node
    }

    /// Re-reads every derived field. `media.class` is fixed for a node's
    /// lifetime, so the audio slot is only created, never dropped.
    pub fn set_props(&mut self, props: BTreeMap<String, String>) {
        let get = |k: &str| props.get(k).cloned().unwrap_or_default();
        if let Some(class) = props.get("media.class") {
            self.media_class = class.clone();
            self.node_type = NodeType::from_media_class(class);
        }
        if props.contains_key("node.name") {
            self.name = get("node.name");
        }
        if props.contains_key("node.description") {
            self.description = get("node.description");
        }
        if props.contains_key("node.nick") {
            self.nick = get("node.nick");
        }
        if let Some(cat) = props.get("media.category") {
            self.is_monitor = cat == "Monitor" || cat == "Manager";
        }
        if let Some(serial) = props.get("object.serial").and_then(|s| s.parse().ok()) {
            self.serial = Some(serial);
        }
        if self.node_type.audio && self.audio.is_none() {
            self.audio = Some(AudioState::default());
        }
        self.props = props;
    }

    pub fn is_sink(&self) -> bool {
        self.node_type.sink
    }

    pub fn is_stream(&self) -> bool {
        self.node_type.stream
    }

    /// A device the launcher and the audio panel list: audio, not a stream.
    pub fn is_device(&self) -> bool {
        self.audio.is_some() && !self.is_stream()
    }

    fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .get(key)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// The application a stream belongs to.
    pub fn app(&self) -> App {
        let own = |k: &str| self.prop(k).map(str::to_owned);
        App {
            name: own("application.name"),
            binary: own("application.process.binary"),
            id: own("application.id"),
            pid: self
                .prop("application.process.id")
                .and_then(|p| p.parse().ok()),
            icon_name: own("application.icon-name"),
            media_name: own("media.name"),
        }
    }

    /// An icon name off the node's own props: the app's for a stream, the
    /// device's for a sink or source.
    pub fn icon_name(&self) -> Option<&str> {
        [
            "application.icon-name",
            "media.icon-name",
            "device.icon-name",
        ]
        .into_iter()
        .find_map(|k| self.prop(k))
    }

    /// The audio panel's label chain: application.name, node.description,
    /// media.name, node.name.
    pub fn label(&self) -> &str {
        self.prop("application.name")
            .or(Some(self.description.as_str()).filter(|s| !s.is_empty()))
            .or_else(|| self.prop("media.name"))
            .unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct App {
    pub name: Option<String>,
    pub binary: Option<String>,
    pub id: Option<String>,
    pub pid: Option<u32>,
    pub icon_name: Option<String>,
    pub media_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LinkState {
    Error(String),
    Unlinked,
    #[default]
    Init,
    Negotiating,
    Allocating,
    Paused,
    Active,
}

/// One port-to-port link. A stream playing stereo into a sink is two.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Link {
    pub id: u32,
    pub output_node: u32,
    pub output_port: u32,
    pub input_node: u32,
    pub input_port: u32,
    pub state: LinkState,
}

/// A link group: every link between one pair of nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGroup {
    pub output_node: u32,
    pub input_node: u32,
    /// The state of the group's lowest-id link.
    pub state: LinkState,
}

/// The four names the `default` metadata object carries. `sink` and
/// `source` are what the session manager resolved and what the shell shows;
/// the configured pair is what a client asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Defaults {
    pub sink: String,
    pub source: String,
    pub configured_sink: String,
    pub configured_source: String,
}

impl Defaults {
    /// One metadata property event. Returns whether a field changed.
    pub fn apply(&mut self, key: Option<&str>, type_: Option<&str>, value: Option<&str>) -> bool {
        let slot = match key {
            Some(KEY_DEFAULT_SINK) => &mut self.sink,
            Some(KEY_DEFAULT_SOURCE) => &mut self.source,
            Some(KEY_CONFIGURED_SINK) => &mut self.configured_sink,
            Some(KEY_CONFIGURED_SOURCE) => &mut self.configured_source,
            _ => return false,
        };
        let name = parse_default_name(type_, value);
        if *slot == name {
            return false;
        }
        *slot = name;
        true
    }
}

/// A default's value is `{"name": "<node.name>"}` typed `Spa:String:JSON`;
/// anything else reads as no default.
pub fn parse_default_name(type_: Option<&str>, value: Option<&str>) -> String {
    let (Some(DEFAULT_TYPE), Some(value)) = (type_, value) else {
        return String::new();
    };
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|v| v.get("name")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The value written for a default: compact JSON, as pactl writes it.
pub fn default_value(name: &str) -> String {
    serde_json::json!({ "name": name }).to_string()
}

/// What the client thread posts, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A node appeared or any of its fields changed; always the whole node.
    Node(Node),
    NodeRemoved(u32),
    Link(Link),
    LinkRemoved(u32),
    Defaults(Defaults),
    /// The initial burst has landed: every node bound so far has its props
    /// and volumes.
    Ready,
    /// The connection dropped. Everything known is gone; a reconnect starts
    /// over with fresh `Node` events and another `Ready`.
    Disconnected,
}

/// What applying an event changed, for consumers that react to a subset
/// (the OSD shows on a default sink's volume or mute moving).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Nothing,
    NodeAdded(u32),
    NodeChanged {
        id: u32,
        volume: bool,
        muted: bool,
        props: bool,
    },
    NodeRemoved(u32),
    Links,
    Defaults {
        sink: bool,
        source: bool,
    },
    Ready,
    Disconnected,
}

/// Writes, executed on the client thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The averaged volume, channel balance kept ([`scaled_volumes`]).
    SetVolume {
        node: u32,
        volume: f32,
    },
    SetVolumes {
        node: u32,
        volumes: Vec<f32>,
    },
    SetMuted {
        node: u32,
        muted: bool,
    },
    /// By `node.name`; `None` clears the configured default. What `pactl set-default-sink` writes.
    SetDefaultSink(Option<String>),
    SetDefaultSource(Option<String>),
}

/// The graph as the shell sees it.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: BTreeMap<u32, Node>,
    pub links: BTreeMap<u32, Link>,
    pub defaults: Defaults,
    pub ready: bool,
}

impl Graph {
    pub fn apply(&mut self, event: Event) -> Change {
        match event {
            Event::Node(node) => {
                let id = node.id;
                match self.nodes.insert(id, node) {
                    None => Change::NodeAdded(id),
                    Some(old) => {
                        let new = &self.nodes[&id];
                        let (old_a, new_a) = (
                            old.audio.unwrap_or_default(),
                            new.audio.clone().unwrap_or_default(),
                        );
                        let volume = old_a.volumes != new_a.volumes;
                        let muted = old_a.muted != new_a.muted;
                        let props = old.props != new.props || old.ready != new.ready;
                        if volume || muted || props || old_a.channels != new_a.channels {
                            Change::NodeChanged {
                                id,
                                volume,
                                muted,
                                props,
                            }
                        } else {
                            Change::Nothing
                        }
                    }
                }
            }
            Event::NodeRemoved(id) => match self.nodes.remove(&id) {
                Some(_) => Change::NodeRemoved(id),
                None => Change::Nothing,
            },
            Event::Link(link) => {
                if self.links.get(&link.id) == Some(&link) {
                    return Change::Nothing;
                }
                self.links.insert(link.id, link);
                Change::Links
            }
            Event::LinkRemoved(id) => match self.links.remove(&id) {
                Some(_) => Change::Links,
                None => Change::Nothing,
            },
            Event::Defaults(defaults) => {
                let sink = defaults.sink != self.defaults.sink;
                let source = defaults.source != self.defaults.source;
                let any = defaults != self.defaults;
                self.defaults = defaults;
                if any {
                    Change::Defaults { sink, source }
                } else {
                    Change::Nothing
                }
            }
            Event::Ready => {
                self.ready = true;
                Change::Ready
            }
            Event::Disconnected => {
                *self = Self::default();
                Change::Disconnected
            }
        }
    }

    /// `PwRegistry::findNodeByName`: an empty name finds nothing.
    pub fn node_by_name(&self, name: &str) -> Option<&Node> {
        if name.is_empty() {
            return None;
        }
        self.nodes.values().find(|n| n.name == name)
    }

    /// `Pipewire.defaultAudioSink`.
    pub fn default_sink(&self) -> Option<&Node> {
        self.node_by_name(&self.defaults.sink)
    }

    /// `Pipewire.defaultAudioSource`.
    pub fn default_source(&self) -> Option<&Node> {
        self.node_by_name(&self.defaults.source)
    }

    /// AudioService's `_find`: an audio device (never a stream) by name.
    pub fn device_by_name(&self, name: &str) -> Option<&Node> {
        self.nodes
            .values()
            .find(|n| n.name == name && n.is_device())
    }

    pub fn link_groups(&self) -> Vec<LinkGroup> {
        let mut groups: BTreeMap<(u32, u32), LinkState> = BTreeMap::new();
        for link in self.links.values() {
            groups
                .entry((link.output_node, link.input_node))
                .or_insert_with(|| link.state.clone());
        }
        groups
            .into_iter()
            .map(|((output_node, input_node), state)| LinkGroup {
                output_node,
                input_node,
                state,
            })
            .collect()
    }

    /// The sink a playback stream is linked into, MediaService's output.
    pub fn stream_target(&self, stream: u32) -> Option<&Node> {
        self.links
            .values()
            .filter(|l| l.output_node == stream)
            .find_map(|l| {
                self.nodes
                    .get(&l.input_node)
                    .filter(|n| n.is_sink() && !n.is_stream())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn sink(id: u32, name: &str) -> Node {
        Node::from_props(
            id,
            props(&[("media.class", "Audio/Sink"), ("node.name", name)]),
        )
    }

    #[test]
    fn volume_curve_is_cubic() {
        assert!((visual_from_linear(0.125) - 0.5).abs() < 1e-6);
        assert!((linear_from_visual(0.5) - 0.125).abs() < 1e-6);
        assert_eq!(visual_from_linear(1.0), 1.0);
        let v = 0.73_f32;
        assert!((visual_from_linear(linear_from_visual(v)) - v).abs() < 1e-6);
    }

    #[test]
    fn scaled_volumes_keeps_balance() {
        let out = scaled_volumes(&[0.2, 0.4], 0.6);
        assert!((out[0] - 0.4).abs() < 1e-6 && (out[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn scaled_volumes_from_silence_sets_flat() {
        assert_eq!(scaled_volumes(&[0.0, 0.0], 0.5), vec![0.5, 0.5]);
        assert_eq!(scaled_volumes(&[0.3, 0.3], 0.0), vec![0.0, 0.0]);
        assert_eq!(scaled_volumes(&[0.3], -1.0), vec![0.0]);
    }

    #[test]
    fn average_of_nothing_is_zero() {
        assert_eq!(average_volume(&[]), 0.0);
        assert_eq!(AudioState::default().volume(), 0.0);
    }

    #[test]
    fn media_class_flags_hold() {
        let t = NodeType::from_media_class("Stream/Output/Audio");
        assert!(t.audio && t.stream && t.sink && !t.source);
        assert_eq!(t.name(), "AudioOutStream");
        assert_eq!(
            NodeType::from_media_class("Audio/Duplex").name(),
            "AudioDuplex"
        );
        assert_eq!(
            NodeType::from_media_class("Audio/Source").name(),
            "AudioSource"
        );
        assert_eq!(
            NodeType::from_media_class("Stream/Input/Audio").name(),
            "AudioInStream"
        );
        assert_eq!(
            NodeType::from_media_class("Video/Source").name(),
            "VideoSource"
        );
        assert_eq!(
            NodeType::from_media_class("Midi/Bridge").name(),
            "Untracked"
        );
    }

    #[test]
    fn audio_slot_only_on_audio_nodes() {
        assert!(sink(1, "a").audio.is_some());
        let video = Node::from_props(2, props(&[("media.class", "Video/Source")]));
        assert!(video.audio.is_none());
        assert!(!video.is_device());
    }

    #[test]
    fn node_props_derive_fields() {
        let n = Node::from_props(
            7,
            props(&[
                ("media.class", "Stream/Output/Audio"),
                ("node.name", "mpv"),
                ("application.name", "mpv"),
                ("application.process.binary", "mpv"),
                ("application.process.id", "4242"),
                ("application.icon-name", "mpv"),
                ("media.name", "Track"),
                ("object.serial", "99"),
            ]),
        );
        assert!(n.is_stream() && n.is_sink() && !n.is_device());
        assert_eq!(n.serial, Some(99));
        let app = n.app();
        assert_eq!(app.pid, Some(4242));
        assert_eq!(app.binary.as_deref(), Some("mpv"));
        assert_eq!(n.icon_name(), Some("mpv"));
    }

    #[test]
    fn monitor_category() {
        let n = Node::from_props(
            1,
            props(&[("media.class", "Audio/Sink"), ("media.category", "Manager")]),
        );
        assert!(n.is_monitor);
    }

    #[test]
    fn label_chain() {
        let mut n = Node::from_props(1, props(&[("node.name", "n"), ("media.name", "m")]));
        assert_eq!(n.label(), "m");
        n.description = "d".into();
        assert_eq!(n.label(), "d");
        n.props.insert("application.name".into(), "a".into());
        assert_eq!(n.label(), "a");
        let bare = Node::from_props(1, props(&[("node.name", "n")]));
        assert_eq!(bare.label(), "n");
    }

    #[test]
    fn default_name_parse() {
        assert_eq!(
            parse_default_name(Some(DEFAULT_TYPE), Some(r#"{ "name": "alsa_output.x" }"#)),
            "alsa_output.x"
        );
        assert_eq!(
            parse_default_name(Some("Spa:Id"), Some(r#"{"name":"x"}"#)),
            ""
        );
        assert_eq!(parse_default_name(Some(DEFAULT_TYPE), Some("garbage")), "");
        assert_eq!(parse_default_name(Some(DEFAULT_TYPE), None), "");
        assert_eq!(default_value("a\"b"), r#"{"name":"a\"b"}"#);
    }

    #[test]
    fn defaults_apply_by_key() {
        let mut d = Defaults::default();
        let v = default_value("s1");
        assert!(d.apply(Some(KEY_DEFAULT_SINK), Some(DEFAULT_TYPE), Some(&v)));
        assert!(!d.apply(Some(KEY_DEFAULT_SINK), Some(DEFAULT_TYPE), Some(&v)));
        assert!(d.apply(
            Some(KEY_CONFIGURED_SOURCE),
            Some(DEFAULT_TYPE),
            Some(&default_value("m"))
        ));
        assert!(!d.apply(Some("default.video.source"), Some(DEFAULT_TYPE), Some(&v)));
        assert!(!d.apply(None, None, None));
        assert_eq!(d.sink, "s1");
        assert_eq!(d.configured_source, "m");
        assert!(d.apply(Some(KEY_DEFAULT_SINK), None, None));
        assert_eq!(d.sink, "");
    }

    #[test]
    fn graph_resolves_default_sink_by_name() {
        let mut g = Graph::default();
        assert_eq!(g.apply(Event::Node(sink(5, "a"))), Change::NodeAdded(5));
        g.apply(Event::Node(sink(6, "b")));
        let defaults = Defaults {
            sink: "b".into(),
            ..Defaults::default()
        };
        assert_eq!(
            g.apply(Event::Defaults(defaults.clone())),
            Change::Defaults {
                sink: true,
                source: false
            }
        );
        assert_eq!(g.apply(Event::Defaults(defaults)), Change::Nothing);
        assert_eq!(g.default_sink().map(|n| n.id), Some(6));
        assert!(g.default_source().is_none());
        assert_eq!(g.apply(Event::NodeRemoved(6)), Change::NodeRemoved(6));
        assert!(g.default_sink().is_none());
    }

    #[test]
    fn graph_reports_volume_and_mute_moves() {
        let mut g = Graph::default();
        let mut n = sink(1, "a");
        g.apply(Event::Node(n.clone()));
        assert_eq!(g.apply(Event::Node(n.clone())), Change::Nothing);
        n.audio = Some(AudioState {
            channels: vec![3, 4],
            volumes: vec![0.5, 0.5],
            muted: false,
        });
        assert_eq!(
            g.apply(Event::Node(n.clone())),
            Change::NodeChanged {
                id: 1,
                volume: true,
                muted: false,
                props: false
            }
        );
        n.audio.as_mut().unwrap().muted = true;
        assert_eq!(
            g.apply(Event::Node(n)),
            Change::NodeChanged {
                id: 1,
                volume: false,
                muted: true,
                props: false
            }
        );
    }

    #[test]
    fn device_lookup_skips_streams() {
        let mut g = Graph::default();
        let stream = Node::from_props(
            3,
            props(&[("media.class", "Stream/Output/Audio"), ("node.name", "x")]),
        );
        g.apply(Event::Node(stream));
        assert!(g.device_by_name("x").is_none());
        assert!(g.node_by_name("x").is_some());
        assert!(g.node_by_name("").is_none());
        g.apply(Event::Node(sink(4, "x")));
        assert_eq!(g.device_by_name("x").map(|n| n.id), Some(4));
    }

    #[test]
    fn links_group_and_target() {
        let mut g = Graph::default();
        g.apply(Event::Node(sink(10, "out")));
        g.apply(Event::Node(Node::from_props(
            20,
            props(&[("media.class", "Stream/Output/Audio")]),
        )));
        let fl = Link {
            id: 30,
            output_node: 20,
            output_port: 1,
            input_node: 10,
            input_port: 2,
            state: LinkState::Active,
        };
        let fr = Link {
            id: 31,
            output_port: 3,
            input_port: 4,
            ..fl.clone()
        };
        assert_eq!(g.apply(Event::Link(fl.clone())), Change::Links);
        assert_eq!(g.apply(Event::Link(fl)), Change::Nothing);
        g.apply(Event::Link(fr));
        assert_eq!(
            g.link_groups(),
            vec![LinkGroup {
                output_node: 20,
                input_node: 10,
                state: LinkState::Active
            }]
        );
        assert_eq!(g.stream_target(20).map(|n| n.id), Some(10));
        assert!(g.stream_target(10).is_none());
        g.apply(Event::LinkRemoved(30));
        g.apply(Event::LinkRemoved(31));
        assert!(g.link_groups().is_empty());
    }

    #[test]
    fn disconnect_clears_everything() {
        let mut g = Graph::default();
        g.apply(Event::Node(sink(1, "a")));
        g.apply(Event::Ready);
        assert!(g.ready);
        assert_eq!(g.apply(Event::Disconnected), Change::Disconnected);
        assert!(g.nodes.is_empty() && !g.ready);
    }
}
