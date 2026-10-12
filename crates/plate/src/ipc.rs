//! Single-instance handoff: a running plate listens on a Unix socket, and
//! later launches (or `plate preview`) send it what to open instead of
//! starting another window.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenRequest {
    pub paths: Vec<PathBuf>,
    /// Frame number as shown in plate (e.g. 1012 for a comp sequence).
    pub frame: Option<i64>,
    pub compare: Option<PathBuf>,
}

/// One socket per app id, so test instances never talk to the real one.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("PLATE_SOCKET") {
        return PathBuf::from(p);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let app = std::env::var("PLATE_APP_ID").unwrap_or_else(|_| "plate".into());
    dir.join(format!("{app}.sock"))
}

/// Hands the request to a running instance. Ok(false) means none is running.
pub fn send(req: &OpenRequest) -> anyhow::Result<bool> {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else { return Ok(false) };
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    let reply = reply.trim();
    if reply == "ok" {
        Ok(true)
    } else {
        anyhow::bail!("{}", reply.trim_start_matches("error: "))
    }
}

/// Starts listening; requests arrive on the returned channel with a
/// one-shot to answer the client. None if another instance owns the socket.
pub fn listen() -> Option<futures::channel::mpsc::UnboundedReceiver<(OpenRequest, std::sync::mpsc::Sender<Result<(), String>>)>> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return None;
    }
    // Nobody answered: any file there is left over from a crash.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("can't listen on {}: {e}", path.display());
            return None;
        }
    };
    let (tx, rx) = futures::channel::mpsc::unbounded();
    std::thread::Builder::new()
        .name("plate-ipc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let reply = match serde_json::from_str::<OpenRequest>(&line) {
                        Ok(req) => {
                            let (done_tx, done_rx) = std::sync::mpsc::channel();
                            if tx.unbounded_send((req, done_tx)).is_err() {
                                "error: plate is shutting down".to_string()
                            } else {
                                match done_rx.recv_timeout(Duration::from_secs(8)) {
                                    Ok(Ok(())) => "ok".to_string(),
                                    Ok(Err(e)) => format!("error: {e}"),
                                    Err(_) => "ok".to_string(),
                                }
                            }
                        }
                        Err(e) => format!("error: bad request: {e}"),
                    };
                    let _ = (&stream).write_all(format!("{reply}\n").as_bytes());
                });
            }
        })
        .ok()?;
    Some(rx)
}

/// Removes our socket on exit so the next launch doesn't have to probe it.
pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
