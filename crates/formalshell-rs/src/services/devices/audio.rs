//! Audio over fs-audio's PipeWire thread: the default sink's
//! volume and mute, and whether a default source exists and is muted. The
//! graph lives on the service thread; writes go back as commands.

use std::cell::RefCell;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use async_channel::{Receiver, Sender};
use fs_audio::{Command, Graph, Node, Visual};
use futures_lite::FutureExt;

use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    /// None while there is no default sink with an audio interface.
    pub volume: Option<Visual>,
    pub muted: bool,
    /// The default sink's description, or its node name.
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
    /// application.name, application.process.binary, application.id and
    /// node.name, what a player is recognised by.
    pub keys: [Option<String>; 4],
    pub label: String,
    /// media.name.
    pub title: String,
    pub volume: Option<Visual>,
    /// The sink it is linked into.
    pub target: String,
}

fn routing(graph: &Graph, streams: bool) -> Routing {
    let key = |n: &Node, k: &str| n.props.get(k).cloned();
    Routing {
        sinks: graph
            .nodes
            .values()
            .filter(|n| !n.is_stream() && n.is_sink() && n.audio.is_some())
            .map(|n| {
                let label = [n.description.as_str(), n.nick.as_str(), n.name.as_str()].into_iter().find(|l| !l.is_empty()).unwrap_or("");
                (n.name.clone(), label.to_owned())
            })
            .collect(),
        streams: graph
            .nodes
            .values()
            .filter(|n| streams && n.is_stream() && n.is_sink() && n.ready)
            .map(|n| StreamInfo {
                id: n.id,
                keys: [key(n, "application.name"), key(n, "application.process.binary"), key(n, "application.id"), Some(n.name.clone())],
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

fn row(n: &Node, label: &str) -> Option<Row> {
    let a = n.audio.as_ref()?;
    Some(Row { id: n.id, name: n.name.clone(), label: label.to_owned(), volume: a.volume(), muted: a.muted })
}

fn device_label(n: &Node) -> &str {
    if n.description.is_empty() { &n.name } else { &n.description }
}

fn lists(graph: &Graph) -> Lists {
    let devices = || graph.nodes.values().filter(|n| n.is_device());
    Lists {
        outputs: devices().filter(|n| n.is_sink()).filter_map(|n| row(n, device_label(n))).collect(),
        inputs: devices().filter(|n| !n.is_sink()).filter_map(|n| row(n, device_label(n))).collect(),
        // AudioModel.isPlaybackStream: a stream feeding a sink.
        streams: graph.nodes.values().filter(|n| n.is_stream() && n.is_sink()).filter_map(|n| row(n, n.label())).collect(),
        sink: graph.default_sink().map(|n| n.id),
        source: graph.default_source().and_then(|n| row(n, device_label(n))),
    }
}

fn snapshot(graph: &Graph) -> Audio {
    let node = graph.default_sink();
    let sink = node.and_then(|n| n.audio.as_ref());
    let source = graph.default_source().and_then(|n| n.audio.as_ref());
    Audio {
        volume: sink.map(|a| a.volume()),
        muted: sink.is_some_and(|a| a.muted),
        name: node
            .filter(|_| sink.is_some())
            .map(|n| if n.description.is_empty() { n.name.clone() } else { n.description.clone() })
            .unwrap_or_default(),
        source: source.is_some(),
        source_muted: source.is_some_and(|a| a.muted),
        source_volume: source.map(|a| a.volume()),
        capturing: graph.capturing(),
        lists: PANEL.load(Ordering::Relaxed).then(|| lists(graph)),
        devices: graph
            .nodes
            .values()
            .filter(|n| n.is_device() && n.audio.is_some())
            .map(|n| (n.name.clone(), device_label(n).to_owned(), n.is_sink()))
            .collect(),
        default_sink: node.filter(|_| sink.is_some()).map(|n| n.name.clone()).unwrap_or_default(),
        default_source: graph.default_source().filter(|n| n.audio.is_some()).map(|n| n.name.clone()).unwrap_or_default(),
    }
}

static PANEL: AtomicBool = AtomicBool::new(false);
/// The media panel is open, or a stream is the picked source.
static ROUTING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static POKE: LazyLock<(Sender<()>, Receiver<()>)> = LazyLock::new(async_channel::unbounded);
static WRITES: LazyLock<(Sender<Command>, Receiver<Command>)> = LazyLock::new(async_channel::unbounded);

/// One of the two reasons to keep the routing lists published: `slot` 0 is
/// the media panel being open, 1 a stream being the picked source.
pub fn routing_wanted(slot: usize, on: bool) {
    if ROUTING[slot].swap(on, Ordering::Relaxed) != on {
        let _ = POKE.0.try_send(());
    }
}

/// A write from the UI thread, carried out on the service thread.
pub fn write(c: Command) {
    let _ = WRITES.0.try_send(c);
}

/// The panel open or closed: its lists are published only while it is.
pub fn panel(open: bool) {
    if PANEL.swap(open, Ordering::Relaxed) != open {
        let _ = POKE.0.try_send(());
    }
}

/// A write from the panel, on the service thread.
pub fn command(c: Command) {
    CLIENT.with_borrow(|client| {
        if let Some(client) = client {
            client.handle.send(c);
        }
    });
}

fn publish(ctx: &Ctx, audio: Audio) {
    ctx.publish(store::Diff::Devices(super::Diff::Audio(audio)));
}

pub async fn run(ctx: Ctx) {
    let (handle, events) = match fs_audio::spawn() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("audio: {err}");
            return publish(&ctx, Audio::default());
        }
    };
    CLIENT.with_borrow_mut(|c| *c = Some(Client { handle, graph: Graph::default() }));
    let mut sent = Routing::default();
    let mut capturing = false;
    loop {
        let event = async { events.recv().await.ok().map(Some) }
            .or(async { POKE.1.recv().await.ok().map(|_| None) })
            .or(async {
                let write = WRITES.1.recv().await.ok()?;
                command(write);
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
            Some((snapshot(&c.graph), routing(&c.graph, ROUTING.iter().any(|r| r.load(Ordering::Relaxed)))))
        });
        if let Some((audio, routed)) = audio {
            if audio.capturing != capturing {
                capturing = audio.capturing;
                super::earbuds::mic(capturing);
            }
            publish(&ctx, audio);
            if routed != sent {
                sent = routed.clone();
                ctx.publish(store::Diff::Media(crate::services::media::Diff::Routing(routed)));
            }
        }
    }
}

fn with_node(source: bool, f: impl FnOnce(&fs_audio::Audio, u32, &fs_audio::AudioState)) {
    CLIENT.with_borrow(|c| {
        let Some(c) = c else { return };
        let node = if source { c.graph.default_source() } else { c.graph.default_sink() };
        if let Some((id, audio)) = node.and_then(|n| Some((n.id, n.audio.as_ref()?))) {
            f(&c.handle, id, audio);
        }
    });
}

/// Held to 0..1: overdrive is the audio panel's, per stream.
fn full_at_most(volume: Visual) -> Visual {
    Visual::new(volume.get().clamp(0.0, 1.0))
}

pub fn set_volume(_: &Ctx, volume: Visual) {
    with_node(false, |h, node, _| h.send(Command::SetVolume { node, volume: full_at_most(volume) }));
}

pub fn set_source_volume(_: &Ctx, volume: Visual) {
    with_node(true, |h, node, _| h.send(Command::SetVolume { node, volume: full_at_most(volume) }));
}

pub fn toggle_mute(_: &Ctx) {
    with_node(false, |h, node, a| h.send(Command::SetMuted { node, muted: !a.muted }));
}

pub fn toggle_source_mute(_: &Ctx) {
    with_node(true, |h, node, a| h.send(Command::SetMuted { node, muted: !a.muted }));
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
