//! Audio over fs-audio's PipeWire thread: the default sink's
//! volume and mute, and whether a default source exists and is muted. The
//! graph lives on the service thread; writes go back as commands. It is the
//! shell's one read of PipeWire: the media panel's routing, the radio's
//! outputs and the lyrics' output latency all come off it.

use std::cell::RefCell;
use std::sync::LazyLock;

use async_channel::{Receiver, Sender};
use fs_audio::{Command, Graph, Node, Visual};
use fs_media::media::StreamKeys;
use futures_lite::FutureExt;

use crate::runtime::Ctx;
use crate::services::wants::{self, Source};
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    /// None while there is no default sink with an audio interface.
    pub volume: Option<Visual>,
    pub muted: bool,
    /// The default sink's label ([`fs_audio::Node::device_label`]).
    pub name: String,
    /// A default source with an audio interface exists.
    pub source: bool,
    pub source_muted: bool,
    /// The default source's volume, while it has one.
    pub source_volume: Option<Visual>,
    /// Something records off a source ([`fs_audio::Graph::capturing`]).
    pub capturing: bool,
    /// The graph the audio panel lists, read only while it is open.
    pub lists: Option<Lists>,
    /// Every device node (the launcher's audio route): its node name, its
    /// label and whether it is a sink. Volumes stay out, so a level change
    /// publishes nothing here.
    pub devices: Vec<(String, String, bool)>,
    /// The default sink's and source's node names.
    pub default_sink: String,
    pub default_source: String,
}

/// One device or stream row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: u32,
    pub name: String,
    pub label: String,
    pub volume: Visual,
    pub muted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lists {
    pub outputs: Vec<Row>,
    pub inputs: Vec<Row>,
    pub streams: Vec<Row>,
    pub sink: Option<u32>,
    /// The default source, with its volume.
    pub source: Option<Row>,
}

/// What the media panel routes by: the sinks a stream can move to, always
/// kept (`media outputs` answers with the panel shut), and every playback
/// stream with what it is linked into, read only while the panel is open or
/// a stream is the picked source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routing {
    /// Node name and label of each sink.
    pub sinks: Vec<(String, String)>,
    pub streams: Vec<StreamInfo>,
    pub default_sink: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo {
    pub id: u32,
    pub keys: StreamKeys,
    pub label: String,
    /// media.name.
    pub title: String,
    pub volume: Option<Visual>,
    /// The sink it is linked into.
    pub target: String,
}

/// The names a playback stream is matched to its player by.
pub fn stream_keys(n: &Node) -> StreamKeys {
    let key = |k: &str| n.props.get(k).cloned();
    StreamKeys {
        app_name: key("application.name"),
        binary: key("application.process.binary"),
        app_id: key("application.id"),
        node_name: Some(n.name.clone()),
    }
}

/// Node name and label of every sink a stream can move to.
pub fn sinks(graph: &Graph) -> Vec<(String, String)> {
    graph.sinks().map(|n| (n.name.clone(), n.device_label())).collect()
}

fn routing(graph: &Graph, streams: bool) -> Routing {
    Routing {
        sinks: sinks(graph),
        streams: graph
            .nodes
            .values()
            .filter(|n| streams && n.is_stream() && n.is_sink() && n.ready)
            .map(|n| StreamInfo {
                id: n.id,
                keys: stream_keys(n),
                label: n.label().to_owned(),
                title: n.props.get("media.name").cloned().unwrap_or_default(),
                volume: n.audio.as_ref().map(|a| a.volume()),
                target: graph.stream_target(n.id).map(|t| t.name.clone()).unwrap_or_default(),
            })
            .collect(),
        default_sink: graph.default_sink().map(|n| n.name.clone()).unwrap_or_default(),
    }
}

struct Client {
    handle: fs_audio::Audio,
    graph: Graph,
}

thread_local! {
    static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
}

fn row(n: &Node, label: String) -> Option<Row> {
    let a = n.audio.as_ref()?;
    Some(Row { id: n.id, name: n.name.clone(), label, volume: a.volume(), muted: a.muted })
}

fn lists(graph: &Graph) -> Lists {
    let devices = || graph.nodes.values().filter(|n| n.is_device());
    Lists {
        outputs: devices().filter(|n| n.is_sink()).filter_map(|n| row(n, n.device_label())).collect(),
        inputs: devices().filter(|n| !n.is_sink()).filter_map(|n| row(n, n.device_label())).collect(),
        // AudioModel.isPlaybackStream: a stream feeding a sink.
        streams: graph.nodes.values().filter(|n| n.is_stream() && n.is_sink()).filter_map(|n| row(n, n.label().to_owned())).collect(),
        sink: graph.default_sink().map(|n| n.id),
        source: graph.default_source().and_then(|n| row(n, n.device_label())),
    }
}

fn snapshot(graph: &Graph, listed: bool) -> Audio {
    let node = graph.default_sink();
    let sink = node.and_then(|n| n.audio.as_ref());
    let source = graph.default_source().and_then(|n| n.audio.as_ref());
    Audio {
        volume: sink.map(|a| a.volume()),
        muted: sink.is_some_and(|a| a.muted),
        name: node.filter(|_| sink.is_some()).map(Node::device_label).unwrap_or_default(),
        source: source.is_some(),
        source_muted: source.is_some_and(|a| a.muted),
        source_volume: source.map(|a| a.volume()),
        capturing: graph.capturing(),
        lists: listed.then(|| lists(graph)),
        devices: graph
            .nodes
            .values()
            .filter(|n| n.is_device() && n.audio.is_some())
            .map(|n| (n.name.clone(), n.device_label(), n.is_sink()))
            .collect(),
        default_sink: node.filter(|_| sink.is_some()).map(|n| n.name.clone()).unwrap_or_default(),
        default_source: graph.default_source().filter(|n| n.audio.is_some()).map(|n| n.name.clone()).unwrap_or_default(),
    }
}

/// A node a write names: the default sink or source, or a row the lists
/// or the routing published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Sink,
    Source,
    Node(u32),
}

/// A write to the graph.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// The default sink and source are held to 0..1, since overdrive is the
    /// audio panel's, per stream; a node takes what it is given.
    Volume(Target, Visual),
    /// A listed node's mute, set outright.
    Mute(u32, bool),
    ToggleMute(Target),
    /// The default sink (false) or source (true), by node name.
    Default { source: bool, name: String },
    MoveStream { stream: u32, sink: String },
}

static CMDS: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread; carried out on the service thread.
pub fn send(c: Cmd) {
    let _ = CMDS.0.try_send(c);
}

/// The graph as the service thread last saw it, `None` with no PipeWire
/// thread at all.
pub fn with_graph<R>(f: impl FnOnce(&Graph) -> R) -> Option<R> {
    CLIENT.with_borrow(|c| c.as_ref().map(|c| f(&c.graph)))
}

fn apply(c: &Client, cmd: Cmd) {
    let node = |target| match target {
        Target::Sink => c.graph.default_sink(),
        Target::Source => c.graph.default_source(),
        Target::Node(id) => c.graph.nodes.get(&id),
    };
    let command = match cmd {
        Cmd::Volume(target, volume) => match node(target).filter(|n| n.audio.is_some()) {
            Some(n) => Command::SetVolume { node: n.id, volume: volume.held_for(&n.node_type) },
            None => return,
        },
        Cmd::Mute(node, muted) => Command::SetMuted { node, muted },
        Cmd::ToggleMute(target) => match node(target).and_then(|n| Some((n.id, n.audio.as_ref()?))) {
            Some((node, a)) => Command::SetMuted { node, muted: !a.muted },
            None => return,
        },
        Cmd::Default { source: false, name } => Command::SetDefaultSink(Some(name)),
        Cmd::Default { source: true, name } => Command::SetDefaultSource(Some(name)),
        Cmd::MoveStream { stream, sink } => Command::MoveStream { stream, sink },
    };
    c.handle.send(command);
}

fn publish(ctx: &Ctx, audio: Audio) {
    ctx.publish(store::Diff::Audio(audio));
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (handle, events) = match fs_audio::spawn() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("audio: {err}");
            return publish(&ctx, Audio::default());
        }
    };
    CLIENT.with_borrow_mut(|c| *c = Some(Client { handle, graph: Graph::default() }));
    let lists_wanted = wants::watch(Source::AudioLists);
    let routing_wanted = wants::watch(Source::AudioRouting);
    let (mut listed, mut routed_streams) = (false, false);
    let mut sent = Routing::default();
    let mut capturing = false;
    loop {
        let event = async { events.recv().await.ok().map(Some) }
            .or(async {
                listed = lists_wanted.recv().await.ok()?;
                Some(None)
            })
            .or(async {
                routed_streams = routing_wanted.recv().await.ok()?;
                Some(None)
            })
            .or(async {
                let cmd = CMDS.1.recv().await.ok()?;
                CLIENT.with_borrow(|c| {
                    if let Some(c) = c {
                        apply(c, cmd);
                    }
                });
                Some(None)
            })
            .await;
        let Some(event) = event else { return };
        let audio = CLIENT.with_borrow_mut(|c| {
            let c = c.as_mut()?;
            if let Some(event) = event {
                c.graph.apply(event);
            }
            // One publish per burst: the initial sync is hundreds of events.
            while let Ok(more) = events.try_recv() {
                c.graph.apply(more);
            }
            Some((snapshot(&c.graph, listed), routing(&c.graph, routed_streams)))
        });
        if let Some((audio, routed)) = audio {
            if audio.capturing != capturing {
                capturing = audio.capturing;
                super::earbuds::send(super::earbuds::Cmd::Mic(capturing));
            }
            publish(&ctx, audio);
            if routed != sent {
                sent = routed.clone();
                ctx.publish(store::Diff::Media(crate::services::media::Diff::Routing(routed)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn node(id: u32, class: &str, name: &str) -> Node {
        let props: BTreeMap<String, String> =
            [("media.class", class), ("node.name", name)].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        Node::from_props(id, props)
    }

    #[test]
    fn every_sink_is_listed_with_no_panel_open() {
        let mut graph = Graph::default();
        for n in [node(1, "Audio/Sink", "alsa_out"), node(2, "Audio/Sink", "smoke_alt"), node(3, "Stream/Output/Audio", "mpv")] {
            graph.apply(fs_audio::Event::Node(n));
        }
        let shut = routing(&graph, false);
        let mut names: Vec<_> = shut.sinks.iter().map(|(n, _)| n.as_str()).collect();
        names.sort();
        assert_eq!(names, ["alsa_out", "smoke_alt"]);
        assert!(shut.streams.is_empty());
    }
}
