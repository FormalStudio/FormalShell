//! `prompter_probe`: holds the system prompter, then answers it with the
//! password on the first line of stdin and stays up past the prompter's own
//! linger.

use std::io::BufRead;
use std::time::Duration;

use fs_auth::prompter::Prompter;
use zeroize::Zeroizing;

fn main() {
    let prompter = Prompter::claim();
    let mut line = Zeroizing::new(String::new());
    std::io::stdin().lock().read_line(&mut line).expect("read stdin");
    prompter.answer(Zeroizing::new(line.trim_end_matches('\n').to_owned()));
    std::thread::sleep(Duration::from_secs(12));
}
