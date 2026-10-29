//! Drives the client thread from a shell, for the NixOS test:
//!
//! fs-audio-probe dump
//! fs-audio-probe set-volume <node.name> <visual>
//! fs-audio-probe mute <node.name> <true|false>
//! fs-audio-probe set-default-sink <node.name>
//! fs-audio-probe wait-stream <app or node name part> <seconds>

use std::process::ExitCode;
use std::time::{Duration, Instant};

use fs_audio::{Command, Event, Graph, Node};
use serde_json::{Value, json};

fn pump(
    rx: &async_channel::Receiver<Event>,
    graph: &mut Graph,
    until: Instant,
    done: impl Fn(&Graph) -> bool,
) -> bool {
    while Instant::now() < until {
        while let Ok(event) = rx.try_recv() {
            graph.apply(event);
        }
        if done(graph) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn node_json(graph: &Graph, n: &Node) -> Value {
    let audio = n.audio.clone().unwrap_or_default();
    json!({
        "id": n.id,
        "name": n.name,
        "description": n.description,
        "media_class": n.media_class,
        "type": n.node_type.name(),
        "is_sink": n.is_sink(),
        "is_stream": n.is_stream(),
        "label": n.label(),
        "icon": n.icon_name(),
        "app": n.app().name,
        "binary": n.app().binary,
        "ready": n.ready,
        "volume": n.audio.as_ref().map(|a| a.volume()),
        "volumes": audio.volumes,
        "channels": audio.channels,
        "muted": audio.muted,
        "target": graph.stream_target(n.id).map(|t| t.name.clone()),
    })
}

fn dump(graph: &Graph) -> Value {
    json!({
        "nodes": graph.nodes.values().map(|n| node_json(graph, n)).collect::<Vec<_>>(),
        "defaults": {
            "sink": graph.defaults.sink,
            "source": graph.defaults.source,
            "configured_sink": graph.defaults.configured_sink,
            "configured_source": graph.defaults.configured_source,
        },
        "default_sink": graph.default_sink().map(|n| n.id),
        "link_groups": graph.link_groups().iter().map(|g| json!([g.output_node, g.input_node])).collect::<Vec<_>>(),
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (audio, rx) = fs_audio::spawn().expect("spawn the pipewire thread");
    let mut graph = Graph::default();
    if !pump(
        &rx,
        &mut graph,
        Instant::now() + Duration::from_secs(10),
        |g| g.ready,
    ) {
        eprintln!("no Ready from pipewire");
        return ExitCode::FAILURE;
    }
    let find = |graph: &Graph, name: &str| graph.node_by_name(name).map(|n| n.id);
    let settle = |graph: &mut Graph| {
        pump(&rx, graph, Instant::now() + Duration::from_secs(1), |_| {
            false
        });
    };
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["dump"] => println!("{}", dump(&graph)),
        ["set-volume", name, v] => {
            let Some(node) = find(&graph, name) else {
                return ExitCode::FAILURE;
            };
            audio.send(Command::SetVolume {
                node,
                volume: v.parse().expect("a number"),
            });
            settle(&mut graph);
        }
        ["mute", name, m] => {
            let Some(node) = find(&graph, name) else {
                return ExitCode::FAILURE;
            };
            audio.send(Command::SetMuted {
                node,
                muted: *m == "true",
            });
            settle(&mut graph);
        }
        ["set-default-sink", name] => {
            audio.send(Command::SetDefaultSink(Some(name.to_string())));
            let ok = pump(
                &rx,
                &mut graph,
                Instant::now() + Duration::from_secs(5),
                |g| g.defaults.sink == *name,
            );
            if !ok {
                return ExitCode::FAILURE;
            }
        }
        ["wait-stream", app, secs] => {
            let until = Instant::now() + Duration::from_secs(secs.parse().expect("seconds"));
            let hit = |g: &Graph| {
                g.nodes
                    .values()
                    .find(|n| {
                        let a = n.app();
                        let names = [
                            a.name.unwrap_or_default(),
                            a.binary.unwrap_or_default(),
                            n.name.clone(),
                        ];
                        n.is_stream()
                            && names.iter().any(|s| s.contains(*app))
                            && g.stream_target(n.id).is_some()
                            && n.audio.as_ref().is_some_and(|a| !a.volumes.is_empty())
                    })
                    .map(|n| n.id)
            };
            if !pump(&rx, &mut graph, until, |g| hit(g).is_some()) {
                println!("{}", dump(&graph));
                return ExitCode::FAILURE;
            }
            let id = hit(&graph).unwrap();
            println!("{}", node_json(&graph, &graph.nodes[&id]));
        }
        _ => {
            eprintln!("usage: fs-audio-probe dump|set-volume|mute|set-default-sink|wait-stream");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
