//! The client thread. pipewire-rs objects are `Rc`-bound to the loop that
//! made them, so the connection lives on a thread of its own running the
//! PipeWire main loop. It posts [`Event`]s over an async channel the service
//! thread awaits, and takes [`Command`]s over a `pipewire::channel` attached
//! to its own loop. It keeps its own [`Graph`] so a write can be computed
//! against the volumes it last saw.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::context::ContextRc;
use pw::core::{CoreRc, PW_ID_CORE};
use pw::device::{Device, DeviceChangeMask, DeviceListener};
use pw::link::{Link as PwLink, LinkListener, LinkState as PwLinkState};
use pw::main_loop::MainLoopRc;
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node as PwNode, NodeChangeMask, NodeListener};
use pw::registry::{GlobalObject, RegistryRc};
use pw::spa;
use pw::types::ObjectType;
use spa::param::{ParamInfo, ParamInfoFlags, ParamType};
use spa::pod::deserialize::PodDeserializer;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, Value, ValueArray};
use spa::sys as ss;
use spa::utils::dict::DictRef;
use spa::utils::result::AsyncSeq;

use crate::{
    AudioState, Command, DEFAULT_TYPE, Defaults, Event, Graph, KEY_CONFIGURED_SINK,
    KEY_CONFIGURED_SOURCE, Link, LinkState, Node, default_value, linear_from_visual,
    scaled_volumes, visual_from_linear,
};

const RETRY: Duration = Duration::from_secs(2);

enum Msg {
    Command(Command),
    Quit,
}

/// The handle the service thread keeps. Dropping it stops and joins the
/// client thread.
pub struct Audio {
    tx: pw::channel::Sender<Msg>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Audio {
    pub fn send(&self, command: Command) {
        let _ = self.tx.send(Msg::Command(command));
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the client thread. It connects to the user's PipeWire and
/// reconnects every two seconds while it cannot; a connection that drops
/// posts [`Event::Disconnected`] first.
pub fn spawn() -> std::io::Result<(Audio, async_channel::Receiver<Event>)> {
    let (events_tx, events_rx) = async_channel::unbounded();
    let (tx, rx) = pw::channel::channel::<Msg>();
    let stop = Arc::new(AtomicBool::new(false));
    let thread = std::thread::Builder::new()
        .name("fs-pipewire".into())
        .spawn({
            let stop = stop.clone();
            move || run(rx, &events_tx, &stop)
        })?;
    Ok((
        Audio {
            tx,
            stop,
            thread: Some(thread),
        },
        events_rx,
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum End {
    Quit,
    Lost,
    Failed,
}

fn run(
    mut rx: pw::channel::Receiver<Msg>,
    events: &async_channel::Sender<Event>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        let (back, end) = session(rx, events);
        rx = back;
        match end {
            End::Quit => return,
            End::Lost => {
                let _ = events.try_send(Event::Disconnected);
            }
            End::Failed => {}
        }
        let until = Instant::now() + RETRY;
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn session(
    rx: pw::channel::Receiver<Msg>,
    events: &async_channel::Sender<Event>,
) -> (pw::channel::Receiver<Msg>, End) {
    let Ok(main_loop) = MainLoopRc::new(None) else {
        return (rx, End::Failed);
    };
    let Ok(context) = ContextRc::new(&main_loop, None) else {
        return (rx, End::Failed);
    };
    let Ok(core) = context.connect_rc(None) else {
        return (rx, End::Failed);
    };
    let Ok(registry) = core.get_registry_rc() else {
        return (rx, End::Failed);
    };

    let end = Rc::new(Cell::new(End::Quit));
    let state = Rc::new(RefCell::new(State::new(events.clone(), core.clone())));

    let _core_listener = core
        .add_listener_local()
        .done({
            let state = Rc::downgrade(&state);
            move |id, seq| {
                if id == PW_ID_CORE
                    && let Some(state) = state.upgrade()
                {
                    state.borrow_mut().on_done(seq);
                }
            }
        })
        .error({
            let main_loop = main_loop.downgrade();
            let end = end.clone();
            move |id, _seq, _res, _message| {
                if id == PW_ID_CORE
                    && let Some(main_loop) = main_loop.upgrade()
                {
                    end.set(End::Lost);
                    main_loop.quit();
                }
            }
        })
        .register();

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let state = Rc::downgrade(&state);
            let registry = registry.downgrade();
            move |global| {
                if let (Some(state), Some(registry)) = (state.upgrade(), registry.upgrade()) {
                    on_global(&state, &registry, global);
                }
            }
        })
        .global_remove({
            let state = Rc::downgrade(&state);
            move |id| {
                if let Some(state) = state.upgrade() {
                    state.borrow_mut().on_global_remove(id);
                }
            }
        })
        .register();

    match core.sync(0) {
        Ok(seq) => state.borrow_mut().sync = Sync::Globals(seq),
        Err(_) => return (rx, End::Failed),
    }

    let attached = rx.attach(main_loop.loop_(), {
        let main_loop = main_loop.downgrade();
        let state = Rc::downgrade(&state);
        let end = end.clone();
        move |msg| match msg {
            Msg::Quit => {
                end.set(End::Quit);
                if let Some(main_loop) = main_loop.upgrade() {
                    main_loop.quit();
                }
            }
            Msg::Command(command) => {
                if let Some(state) = state.upgrade() {
                    state.borrow_mut().command(command);
                }
            }
        }
    });

    main_loop.run();

    let rx = attached.deattach();
    state.borrow_mut().release();
    (rx, end.get())
}

/// The initial burst is two round trips: the first lands every global, and
/// the binds made while handling them land their info and params before the
/// second.
enum Sync {
    Start,
    Globals(AsyncSeq),
    Binds(AsyncSeq),
    Done,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct VolumeProps {
    volumes: Vec<f32>,
    channels: Vec<u32>,
    mute: bool,
    step: Option<f32>,
}

#[derive(Default)]
struct Hw {
    device: Option<u32>,
    route_device: Option<i32>,
    pro_audio: bool,
    /// The last volumes the server reported, for the device path's step check.
    server: Vec<f32>,
    step: Option<f32>,
}

struct DeviceEntry {
    proxy: Device,
    _listener: DeviceListener,
    /// `card.profile.device` to the route's index and props.
    routes: HashMap<i32, (i32, VolumeProps)>,
    /// A route write is in flight; the next Route param acknowledges it.
    waiting: bool,
    /// Node id to the volumes held back while `waiting`.
    pending: HashMap<u32, Vec<f32>>,
    written: HashMap<u32, Vec<f32>>,
}

struct State {
    events: async_channel::Sender<Event>,
    core: CoreRc,
    sync: Sync,
    graph: Graph,
    nodes: HashMap<u32, (PwNode, NodeListener)>,
    hw: HashMap<u32, Hw>,
    devices: HashMap<u32, DeviceEntry>,
    links: HashMap<u32, (PwLink, LinkListener)>,
    metadata: Option<(u32, Metadata, MetadataListener)>,
}

fn readable(params: &[ParamInfo], kind: ParamType) -> bool {
    params
        .iter()
        .any(|p| p.id() == kind && p.flags().contains(ParamInfoFlags::READ))
}

fn dict(props: Option<&DictRef>) -> BTreeMap<String, String> {
    props
        .map(|d| {
            d.iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn on_global(state: &Rc<RefCell<State>>, registry: &RegistryRc, global: &GlobalObject<&DictRef>) {
    let weak = Rc::downgrade(state);
    match global.type_ {
        ObjectType::Node => {
            let Ok(proxy) = registry.bind::<PwNode, _>(global) else {
                return;
            };
            let id = global.id;
            let listener = proxy
                .add_listener_local()
                .info({
                    let weak = weak.clone();
                    move |info| {
                        let mask = info.change_mask();
                        let props = mask
                            .contains(NodeChangeMask::PROPS)
                            .then(|| dict(info.props()));
                        let enumerate = mask.contains(NodeChangeMask::PARAMS)
                            && readable(info.params(), ParamType::Props);
                        with(&weak, |s| s.on_node_info(id, props, enumerate));
                    }
                })
                .param({
                    let weak = weak.clone();
                    move |_seq, kind, index, _next, pod| {
                        if kind == ParamType::Props
                            && index == 0
                            && let Some(props) = pod.and_then(parse_volume_props)
                        {
                            with(&weak, |s| s.on_node_props(id, props));
                        }
                    }
                })
                .register();
            // The subscription lands the first Props before Ready; a stream's
            // client-node adds Props after binding, which only the info
            // event's PARAMS change announces.
            proxy.subscribe_params(&[ParamType::Props]);
            let node = Node::from_props(id, dict(global.props));
            let mut s = state.borrow_mut();
            s.nodes.insert(id, (proxy, listener));
            let hw = s.hw.entry(id).or_default();
            hw.device = node.props.get("device.id").and_then(|d| d.parse().ok());
            s.emit(Event::Node(node));
        }
        ObjectType::Device => {
            let Ok(proxy) = registry.bind::<Device, _>(global) else {
                return;
            };
            let id = global.id;
            let listener = proxy
                .add_listener_local()
                .info({
                    let weak = weak.clone();
                    move |info| {
                        if info.change_mask().contains(DeviceChangeMask::PARAMS)
                            && readable(info.params(), ParamType::Route)
                        {
                            with(&weak, |s| {
                                if let Some(entry) = s.devices.get(&id) {
                                    entry
                                        .proxy
                                        .enum_params(0, Some(ParamType::Route), 0, u32::MAX);
                                }
                            });
                        }
                    }
                })
                .param(move |_seq, kind, index, _next, pod| {
                    if kind == ParamType::Route
                        && let Some(route) = pod.and_then(parse_route)
                    {
                        with(&weak, |s| s.on_route(id, index, route));
                    }
                })
                .register();
            proxy.subscribe_params(&[ParamType::Route]);
            let entry = DeviceEntry {
                proxy,
                _listener: listener,
                routes: HashMap::new(),
                waiting: false,
                pending: HashMap::new(),
                written: HashMap::new(),
            };
            state.borrow_mut().devices.insert(id, entry);
        }
        ObjectType::Link => {
            let Ok(proxy) = registry.bind::<PwLink, _>(global) else {
                return;
            };
            let listener = proxy
                .add_listener_local()
                .info(move |info| {
                    let state = match info.state() {
                        PwLinkState::Error(e) => LinkState::Error(e.to_owned()),
                        PwLinkState::Unlinked => LinkState::Unlinked,
                        PwLinkState::Init => LinkState::Init,
                        PwLinkState::Negotiating => LinkState::Negotiating,
                        PwLinkState::Allocating => LinkState::Allocating,
                        PwLinkState::Paused => LinkState::Paused,
                        PwLinkState::Active => LinkState::Active,
                    };
                    let link = Link {
                        id: info.id(),
                        output_node: info.output_node_id(),
                        output_port: info.output_port_id(),
                        input_node: info.input_node_id(),
                        input_port: info.input_port_id(),
                        state,
                    };
                    with(&weak, |s| s.emit(Event::Link(link)));
                })
                .register();
            state
                .borrow_mut()
                .links
                .insert(global.id, (proxy, listener));
        }
        ObjectType::Metadata => {
            if global.props.and_then(|p| p.get("metadata.name")) != Some("default") {
                return;
            }
            let Ok(proxy) = registry.bind::<Metadata, _>(global) else {
                return;
            };
            let listener = proxy
                .add_listener_local()
                .property(move |_subject, key, type_, value| {
                    with(&weak, |s| {
                        let mut defaults = s.graph.defaults.clone();
                        if defaults.apply(key, type_, value) {
                            s.emit(Event::Defaults(defaults));
                        }
                    });
                    0
                })
                .register();
            state.borrow_mut().metadata = Some((global.id, proxy, listener));
        }
        _ => {}
    }
}

fn with(state: &Weak<RefCell<State>>, f: impl FnOnce(&mut State)) {
    if let Some(state) = state.upgrade() {
        f(&mut state.borrow_mut());
    }
}

impl State {
    fn new(events: async_channel::Sender<Event>, core: CoreRc) -> Self {
        Self {
            events,
            core,
            sync: Sync::Start,
            graph: Graph::default(),
            nodes: HashMap::new(),
            hw: HashMap::new(),
            devices: HashMap::new(),
            links: HashMap::new(),
            metadata: None,
        }
    }

    fn emit(&mut self, event: Event) {
        self.graph.apply(event.clone());
        let _ = self.events.try_send(event);
    }

    fn emit_node(&mut self, id: u32, f: impl FnOnce(&mut Node)) {
        let Some(mut node) = self.graph.nodes.get(&id).cloned() else {
            return;
        };
        f(&mut node);
        self.emit(Event::Node(node));
    }

    /// Whether the node takes its volume from the device route.
    fn device_route(&self, id: u32) -> Option<(u32, i32)> {
        let hw = self.hw.get(&id)?;
        let (device, route) = (hw.device?, hw.route_device?);
        if hw.pro_audio || !self.devices.get(&device)?.routes.contains_key(&route) {
            return None;
        }
        Some((device, route))
    }

    fn on_done(&mut self, seq: AsyncSeq) {
        match self.sync {
            Sync::Globals(want) if want == seq => {
                self.sync = match self.core.sync(0) {
                    Ok(next) => Sync::Binds(next),
                    Err(_) => Sync::Done,
                };
            }
            Sync::Binds(want) if want == seq => {
                self.sync = Sync::Done;
                self.emit(Event::Ready);
            }
            _ => {}
        }
    }

    fn on_global_remove(&mut self, id: u32) {
        if self.nodes.remove(&id).is_some() {
            self.hw.remove(&id);
            self.emit(Event::NodeRemoved(id));
        } else if self.links.remove(&id).is_some() {
            self.emit(Event::LinkRemoved(id));
        } else if self.devices.remove(&id).is_none()
            && self.metadata.as_ref().is_some_and(|m| m.0 == id)
        {
            self.metadata = None;
            self.emit(Event::Defaults(Defaults::default()));
        }
    }

    fn on_node_info(&mut self, id: u32, props: Option<BTreeMap<String, String>>, enumerate: bool) {
        if enumerate && let Some((proxy, _)) = self.nodes.get(&id) {
            proxy.enum_params(0, Some(ParamType::Props), 0, u32::MAX);
        }
        let Some(props) = props else { return };
        let hw = self.hw.entry(id).or_default();
        hw.pro_audio = props
            .get("device.profile.pro")
            .is_some_and(|p| p == "true" || p == "1");
        hw.device = props.get("device.id").and_then(|d| d.parse().ok());
        hw.route_device = props
            .get("card.profile.device")
            .and_then(|d| d.parse().ok());
        let route = self.device_route(id);
        let from_device = route.and_then(|(device, route)| {
            Some(self.devices.get(&device)?.routes.get(&route)?.1.clone())
        });
        self.emit_node(id, |node| {
            node.set_props(props);
            node.ready = true;
            if let (Some(audio), Some(vp)) = (node.audio.as_mut(), from_device) {
                apply_volume_props(audio, &vp);
            }
        });
    }

    fn on_node_props(&mut self, id: u32, props: VolumeProps) {
        if self.device_route(id).is_some() {
            return;
        }
        self.update_volume_props(id, props);
    }

    fn update_volume_props(&mut self, id: u32, props: VolumeProps) {
        if props.volumes.len() != props.channels.len() {
            return;
        }
        if let Some(hw) = self.hw.get_mut(&id) {
            hw.server = props.volumes.clone();
            hw.step = props.step;
        }
        self.emit_node(id, |node| {
            if let Some(audio) = node.audio.as_mut() {
                apply_volume_props(audio, &props);
            }
        });
    }

    fn on_route(
        &mut self,
        device: u32,
        index: u32,
        (route_device, route_index, props): (i32, i32, VolumeProps),
    ) {
        let Some(entry) = self.devices.get_mut(&device) else {
            return;
        };
        if index == 0 {
            entry.routes.clear();
        }
        entry
            .routes
            .insert(route_device, (route_index, props.clone()));
        let flush = if entry.waiting {
            entry.waiting = false;
            std::mem::take(&mut entry.pending)
        } else {
            HashMap::new()
        };
        for (node, volumes) in flush {
            if self.devices[&device].written.get(&node) != Some(&volumes) {
                self.write_route(
                    device,
                    route_device,
                    channel_volumes(&volumes),
                    Some((node, volumes)),
                );
            }
        }
        let nodes: Vec<u32> = self
            .hw
            .iter()
            .filter(|(_, hw)| hw.device == Some(device) && hw.route_device == Some(route_device))
            .map(|(id, _)| *id)
            .collect();
        for node in nodes {
            if self.device_route(node).is_some() {
                self.update_volume_props(node, props.clone());
            }
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::SetVolume { node, volume } => {
                let Some(current) = self.graph.nodes.get(&node).and_then(|n| n.audio.as_ref())
                else {
                    return;
                };
                let volumes = scaled_volumes(&current.volumes, volume);
                self.set_volumes(node, volumes);
            }
            Command::SetVolumes { node, volumes } => self.set_volumes(node, volumes),
            Command::SetMuted { node, muted } => self.set_muted(node, muted),
            Command::SetDefaultSink(name) => self.set_default(KEY_CONFIGURED_SINK, name, true),
            Command::SetDefaultSource(name) => self.set_default(KEY_CONFIGURED_SOURCE, name, false),
        }
    }

    fn set_volumes(&mut self, id: u32, volumes: Vec<f32>) {
        let Some(audio) = self.graph.nodes.get(&id).and_then(|n| n.audio.as_ref()) else {
            return;
        };
        let volumes: Vec<f32> = volumes.into_iter().map(|v| v.max(0.0)).collect();
        if volumes == audio.volumes || volumes.len() != audio.volumes.len() {
            return;
        }
        if let Some((device, route)) = self.device_route(id) {
            let entry = &self.devices[&device];
            if entry.waiting {
                self.devices
                    .get_mut(&device)
                    .unwrap()
                    .pending
                    .insert(id, volumes.clone());
            } else {
                let hw = &self.hw[&id];
                // Below the route's step the device sends no Route param back,
                // which would leave `waiting` set for good.
                let significant = hw.server.is_empty()
                    || hw.step.is_none()
                    || volumes
                        .iter()
                        .zip(&hw.server)
                        .any(|(t, s)| *t == 0.0 || (t - s).abs() >= hw.step.unwrap_or(0.0));
                if !significant {
                    return;
                }
                self.write_route(
                    device,
                    route,
                    channel_volumes(&volumes),
                    Some((id, volumes.clone())),
                );
            }
        } else {
            let pod = serialize(&props_value(channel_volumes(&volumes)));
            if let (Some(pod), Some((proxy, _))) = (pod, self.nodes.get(&id)) {
                proxy.set_param(
                    ParamType::Props,
                    0,
                    Pod::from_bytes(&pod).expect("serialized pod"),
                );
            }
        }
        self.emit_node(id, |node| {
            if let Some(audio) = node.audio.as_mut() {
                audio.volumes = volumes;
            }
        });
    }

    fn set_muted(&mut self, id: u32, muted: bool) {
        let Some(audio) = self.graph.nodes.get(&id).and_then(|n| n.audio.as_ref()) else {
            return;
        };
        if audio.muted == muted {
            return;
        }
        if let Some((device, route)) = self.device_route(id) {
            self.write_route(
                device,
                route,
                vec![Property::new(ss::SPA_PROP_mute, Value::Bool(muted))],
                None,
            );
        } else {
            let pod = serialize(&props_value(vec![Property::new(
                ss::SPA_PROP_mute,
                Value::Bool(muted),
            )]));
            if let (Some(pod), Some((proxy, _))) = (pod, self.nodes.get(&id)) {
                proxy.set_param(
                    ParamType::Props,
                    0,
                    Pod::from_bytes(&pod).expect("serialized pod"),
                );
            }
        }
        self.emit_node(id, |node| {
            if let Some(audio) = node.audio.as_mut() {
                audio.muted = muted;
            }
        });
    }

    fn write_route(
        &mut self,
        device: u32,
        route_device: i32,
        props: Vec<Property>,
        volumes: Option<(u32, Vec<f32>)>,
    ) {
        let Some(entry) = self.devices.get_mut(&device) else {
            return;
        };
        let Some((route_index, _)) = entry.routes.get(&route_device) else {
            return;
        };
        let route = Value::Object(Object {
            type_: ss::SPA_TYPE_OBJECT_ParamRoute,
            id: ss::SPA_PARAM_Route,
            properties: vec![
                Property::new(ss::SPA_PARAM_ROUTE_device, Value::Int(route_device)),
                Property::new(ss::SPA_PARAM_ROUTE_index, Value::Int(*route_index)),
                Property::new(ss::SPA_PARAM_ROUTE_props, props_value(props)),
                Property::new(ss::SPA_PARAM_ROUTE_save, Value::Bool(true)),
            ],
        });
        let Some(bytes) = serialize(&route) else {
            return;
        };
        entry.proxy.set_param(
            ParamType::Route,
            0,
            Pod::from_bytes(&bytes).expect("serialized pod"),
        );
        if let Some((node, volumes)) = volumes {
            entry.written.insert(node, volumes);
            entry.waiting = true;
        }
    }

    /// Only an audio node of the right
    /// direction, and nothing written when the name is already configured.
    fn set_default(&mut self, key: &str, name: Option<String>, sink: bool) {
        let configured = if sink {
            &self.graph.defaults.configured_sink
        } else {
            &self.graph.defaults.configured_source
        };
        let name = name.unwrap_or_default();
        if &name == configured {
            return;
        }
        if !name.is_empty() {
            let Some(node) = self.graph.device_by_name(&name) else {
                return;
            };
            let fits = if sink {
                node.node_type.sink
            } else {
                node.node_type.source
            };
            if !fits {
                return;
            }
        }
        let Some((_, proxy, _)) = &self.metadata else {
            return;
        };
        if name.is_empty() {
            proxy.set_property(PW_ID_CORE, key, Some(DEFAULT_TYPE), None);
        } else {
            proxy.set_property(
                PW_ID_CORE,
                key,
                Some(DEFAULT_TYPE),
                Some(&default_value(&name)),
            );
        }
    }

    /// Drops every proxy while the core they belong to is still alive.
    fn release(&mut self) {
        self.metadata = None;
        self.links.clear();
        self.nodes.clear();
        self.devices.clear();
    }
}

fn apply_volume_props(audio: &mut AudioState, props: &VolumeProps) {
    audio.channels = props.channels.clone();
    audio.volumes = props.volumes.clone();
    audio.muted = props.mute;
}

fn channel_volumes(volumes: &[f32]) -> Vec<Property> {
    vec![Property::new(
        ss::SPA_PROP_channelVolumes,
        Value::ValueArray(ValueArray::Float(
            volumes.iter().map(|v| linear_from_visual(*v)).collect(),
        )),
    )]
}

fn props_value(properties: Vec<Property>) -> Value {
    Value::Object(Object {
        type_: ss::SPA_TYPE_OBJECT_Props,
        id: ss::SPA_PARAM_Props,
        properties,
    })
}

fn serialize(value: &Value) -> Option<Vec<u8>> {
    PodSerializer::serialize(Cursor::new(Vec::new()), value)
        .ok()
        .map(|(c, _)| c.into_inner())
}

fn value_of(pod: &Pod) -> Option<Value> {
    PodDeserializer::deserialize_any_from(pod.as_bytes())
        .ok()
        .map(|(_, v)| v)
}

fn parse_volume_props(pod: &Pod) -> Option<VolumeProps> {
    volume_props(&value_of(pod)?)
}

/// Parses the SPA pod's volume props, the default channel layouts
/// included.
fn volume_props(value: &Value) -> Option<VolumeProps> {
    let Value::Object(object) = value else {
        return None;
    };
    let mut out = VolumeProps::default();
    for prop in &object.properties {
        match (prop.key, &prop.value) {
            (ss::SPA_PROP_channelVolumes, Value::ValueArray(ValueArray::Float(v))) => {
                out.volumes = v.iter().map(|l| visual_from_linear(*l)).collect();
            }
            (ss::SPA_PROP_channelMap, Value::ValueArray(ValueArray::Id(ids))) => {
                out.channels = ids.iter().map(|id| id.0).collect();
            }
            (ss::SPA_PROP_mute, Value::Bool(mute)) => out.mute = *mute,
            (ss::SPA_PROP_volumeStep, Value::Float(step)) => out.step = Some(*step),
            _ => {}
        }
    }
    if out.channels.is_empty() {
        out.channels = default_layout(out.volumes.len());
    }
    Some(out)
}

fn default_layout(n: usize) -> Vec<u32> {
    use ss::{
        SPA_AUDIO_CHANNEL_FC as FC, SPA_AUDIO_CHANNEL_FL as FL, SPA_AUDIO_CHANNEL_FR as FR,
        SPA_AUDIO_CHANNEL_LFE as LFE, SPA_AUDIO_CHANNEL_MONO as MONO, SPA_AUDIO_CHANNEL_RL as RL,
        SPA_AUDIO_CHANNEL_RR as RR, SPA_AUDIO_CHANNEL_SL as SL, SPA_AUDIO_CHANNEL_SR as SR,
    };
    match n {
        1 => vec![MONO],
        2 => vec![FL, FR],
        3 => vec![FL, FR, LFE],
        4 => vec![FL, FR, RL, RR],
        5 => vec![FL, FR, FC, SL, SR],
        6 => vec![FL, FR, FC, LFE, SL, SR],
        7 => vec![FL, FR, FC, RL, RR, SL, SR],
        8 => vec![FL, FR, FC, LFE, RL, RR, SL, SR],
        _ => Vec::new(),
    }
}

fn parse_route(pod: &Pod) -> Option<(i32, i32, VolumeProps)> {
    let Value::Object(object) = value_of(pod)? else {
        return None;
    };
    let (mut device, mut index, mut props) = (None, None, None);
    for prop in &object.properties {
        match (prop.key, &prop.value) {
            (ss::SPA_PARAM_ROUTE_device, Value::Int(d)) => device = Some(*d),
            (ss::SPA_PARAM_ROUTE_index, Value::Int(i)) => index = Some(*i),
            (ss::SPA_PARAM_ROUTE_props, v) => props = volume_props(v),
            _ => {}
        }
    }
    Some((device?, index?, props?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: &Value) -> Value {
        let bytes = serialize(value).unwrap();
        value_of(Pod::from_bytes(&bytes).unwrap()).unwrap()
    }

    #[test]
    fn props_pod_round_trips_on_the_cubic_scale() {
        let value = props_value(vec![
            Property::new(
                ss::SPA_PROP_channelVolumes,
                Value::ValueArray(ValueArray::Float(vec![0.125, 1.0])),
            ),
            Property::new(ss::SPA_PROP_mute, Value::Bool(true)),
            Property::new(ss::SPA_PROP_volumeStep, Value::Float(0.01)),
        ]);
        let props = volume_props(&roundtrip(&value)).unwrap();
        assert!((props.volumes[0] - 0.5).abs() < 1e-6);
        assert_eq!(props.volumes[1], 1.0);
        assert!(props.mute);
        assert_eq!(props.step, Some(0.01));
        assert_eq!(
            props.channels,
            vec![ss::SPA_AUDIO_CHANNEL_FL, ss::SPA_AUDIO_CHANNEL_FR]
        );
    }

    #[test]
    fn route_pod_parses() {
        let route = Value::Object(Object {
            type_: ss::SPA_TYPE_OBJECT_ParamRoute,
            id: ss::SPA_PARAM_Route,
            properties: vec![
                Property::new(ss::SPA_PARAM_ROUTE_device, Value::Int(3)),
                Property::new(ss::SPA_PARAM_ROUTE_index, Value::Int(7)),
                Property::new(
                    ss::SPA_PARAM_ROUTE_props,
                    props_value(channel_volumes(&[0.5])),
                ),
                Property::new(ss::SPA_PARAM_ROUTE_save, Value::Bool(true)),
            ],
        });
        let (device, index, props) =
            parse_route(Pod::from_bytes(&serialize(&route).unwrap()).unwrap()).unwrap();
        assert_eq!((device, index), (3, 7));
        assert!((props.volumes[0] - 0.5).abs() < 1e-6);
        assert_eq!(props.channels, vec![ss::SPA_AUDIO_CHANNEL_MONO]);
    }

    #[test]
    fn unknown_layouts_stay_empty() {
        assert!(default_layout(0).is_empty());
        assert!(default_layout(9).is_empty());
        assert_eq!(default_layout(6).len(), 6);
    }
}
