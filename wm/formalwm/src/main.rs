use shojiwm_rs::prelude::*;

fn main() -> std::process::ExitCode {
    run_config(|| {
        COMPOSITOR.rendering.composition(|_| OutputStack::default_stacking());
    })
}
