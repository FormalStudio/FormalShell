//! `formalshell-ipc <call|show|wait|listen|prop> ...`: the command line
//! client for the shell's IPC, printing replies as
//! `crates/formalshell-rs/tests/ipc-golden.jsonl` fixes them and exiting as
//! it exits.

// Only the runtime-dir half of the module is used here.
#[allow(dead_code)]
#[path = "../paths.rs"]
mod paths;

#[path = "../ipc"]
mod ipc {
    pub mod cli;
    pub mod client;
    pub mod wire;
}

fn main() {
    let argv: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    std::process::exit(ipc::client::run(&argv));
}
