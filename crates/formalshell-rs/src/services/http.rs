//! Every HTTP request the shell makes. curl does the transfer (one
//! binary, TLS from the system); this is the only place its argv is built.
//! The status trails the body on its own line, so a reply carries both: a
//! transport failure (no route, timeout, a body past the size cap, curl
//! missing) is an `Err`, and a status past 399 is an `Ok` the caller reads.

use std::path::PathBuf;
use std::time::Duration;

use async_io::Timer;
use futures_lite::FutureExt;

#[derive(Clone, Debug, Default)]
pub struct Request {
    url: String,
    max_time: u32,
    connect_timeout: Option<u32>,
    max_filesize: Option<u64>,
    headers: Vec<String>,
    https_only: bool,
    follow: Option<u32>,
    retry: bool,
    compressed: bool,
    query: Vec<(String, String)>,
    body: Option<String>,
    output: Option<PathBuf>,
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The program is not installed.
    Missing,
    /// Never reached the server, ran out `max_time`, or broke the size cap.
    Transport,
}

impl Response {
    /// Below 400, as curl's `--fail` counts it.
    pub fn ok(&self) -> bool {
        self.status < 400
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl Request {
    pub fn get(url: impl Into<String>, max_time: u32) -> Self {
        Self { url: url.into(), max_time, ..Self::default() }
    }

    pub fn connect_timeout(mut self, secs: u32) -> Self {
        self.connect_timeout = Some(secs);
        self
    }

    pub fn max_filesize(mut self, bytes: u64) -> Self {
        self.max_filesize = Some(bytes);
        self
    }

    /// One `Name: value` line.
    pub fn header(mut self, line: impl Into<String>) -> Self {
        self.headers.push(line.into());
        self
    }

    pub fn user_agent(self, agent: &str) -> Self {
        self.header(format!("User-Agent: {agent}"))
    }

    pub fn https_only(mut self) -> Self {
        self.https_only = true;
        self
    }

    /// Follow redirects, at most `max` of them.
    pub fn follow(mut self, max: u32) -> Self {
        self.follow = Some(max);
        self
    }

    /// One retry, at once, on a transient failure.
    pub fn retry_once(mut self) -> Self {
        self.retry = true;
        self
    }

    pub fn compressed(mut self) -> Self {
        self.compressed = true;
        self
    }

    pub fn query(mut self, key: &str, value: &str) -> Self {
        self.query.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Makes it a POST.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Writes the body to a file instead of returning it.
    pub fn output(mut self, path: impl Into<PathBuf>) -> Self {
        self.output = Some(path.into());
        self
    }

    fn argv(&self) -> Vec<String> {
        let mut a: Vec<String> = ["--silent", "--show-error", "--max-time"].map(String::from).to_vec();
        a.push(self.max_time.to_string());
        if let Some(c) = self.connect_timeout {
            a.extend(["--connect-timeout".into(), c.to_string()]);
        }
        if let Some(n) = self.max_filesize {
            a.extend(["--max-filesize".into(), n.to_string()]);
        }
        if self.https_only {
            a.extend(["--proto".into(), "=https".into()]);
        }
        if let Some(n) = self.follow {
            a.extend(["--location".into(), "--max-redirs".into(), n.to_string()]);
        }
        if self.retry {
            a.extend(["--retry", "1", "--retry-delay", "0"].map(String::from));
        }
        if self.compressed {
            a.push("--compressed".into());
        }
        for h in &self.headers {
            a.extend(["-H".into(), h.clone()]);
        }
        if let Some(b) = &self.body {
            a.extend(["-d".into(), b.clone()]);
        }
        if !self.query.is_empty() {
            a.push("--get".into());
            for (k, v) in &self.query {
                a.extend(["--data-urlencode".into(), format!("{k}={v}")]);
            }
        }
        if let Some(path) = &self.output {
            a.extend(["--output".into(), path.to_string_lossy().into_owned()]);
        }
        a.extend(["-w".into(), "\n%{http_code}".into(), "--".into(), self.url.clone()]);
        a
    }
}

/// Dropping the future kills the transfer.
pub async fn send(req: &Request) -> Result<Response, Failure> {
    // Past curl's own cap only if curl itself hangs.
    let outer = Duration::from_secs(u64::from(req.max_time) * (u64::from(req.retry) + 1) + 5);
    let run = async {
        crate::services::proc::command("curl")
            .args(req.argv())
            .stdin(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
    };
    let expired = async {
        Timer::after(outer).await;
        Err(std::io::Error::from(std::io::ErrorKind::TimedOut))
    };
    let out = match run.or(expired).await {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Failure::Missing),
        Err(_) => return Err(Failure::Transport),
    };
    if !out.status.success() {
        return Err(Failure::Transport);
    }
    let mut body = out.stdout;
    let cut = body.iter().rposition(|b| *b == b'\n').ok_or(Failure::Transport)?;
    let status = std::str::from_utf8(&body[cut + 1..]).ok().and_then(|s| s.trim().parse().ok()).ok_or(Failure::Transport)?;
    body.truncate(cut);
    Ok(Response { status, body })
}

/// The reply of a request that got through and was not refused, or `None`.
pub async fn get(req: &Request) -> Option<Response> {
    send(req).await.ok().filter(Response::ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_carries_each_setting_before_the_url() {
        let a = Request::get("https://x.test/a", 8)
            .connect_timeout(4)
            .max_filesize(65536)
            .https_only()
            .user_agent("UA")
            .argv();
        assert_eq!(a.last().map(String::as_str), Some("https://x.test/a"));
        let has = |pair: [&str; 2]| a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["--max-time", "8"]) && has(["--connect-timeout", "4"]) && has(["--max-filesize", "65536"]));
        assert!(has(["--proto", "=https"]) && has(["-H", "User-Agent: UA"]));
    }

    #[test]
    fn a_query_becomes_a_get_with_encoded_fields() {
        let a = Request::get("https://x.test/a", 5).query("q", "a b").argv();
        assert!(a.contains(&"--get".to_owned()) && a.contains(&"q=a b".to_owned()));
    }
}
