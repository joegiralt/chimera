//! The simulator's console over WebSocket, for the browser editor: one text
//! message per console line, both ways. A browser can't open the console's
//! TCP socket. One browser at a time: a new one replaces the old, as the
//! desktop console does with its clients.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

const LISTEN: &str = "127.0.0.1:7342";
const CONSOLE: &str = "127.0.0.1:7341";
/// How long the socket's reader waits before looking at the console's lines.
/// How long a browser has to finish its upgrade request.
const HANDSHAKE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);

pub fn run(args: &[String]) -> Result<(), String> {
    let flag = |name: &str, default: &str| match args.iter().position(|a| a == name) {
        Some(i) => args
            .get(i + 1)
            .cloned()
            .ok_or(format!("{name} needs a value")),
        None => Ok(default.to_owned()),
    };
    let (listen, console) = (flag("--listen", LISTEN)?, flag("--console", CONSOLE)?);
    let listener = TcpListener::bind(&listen).map_err(|e| format!("{listen}: {e}"))?;
    eprintln!("ed-bridge: ws://{listen} <-> {console}");
    serve(listener, &console);
    Ok(())
}

/// Accept browsers on `listener`, each bridged to `console`; never returns.
pub fn serve(listener: TcpListener, console: &str) {
    let current: Arc<Mutex<Option<TcpStream>>> = Arc::default();
    for stream in listener.incoming().flatten() {
        let Ok(mine) = stream.try_clone() else {
            continue;
        };
        // The new browser replaces the old: close the old one's socket.
        if let Some(old) = current.lock().unwrap().replace(mine) {
            let _ = old.shutdown(Shutdown::Both);
        }
        let console = console.to_owned();
        std::thread::spawn(move || {
            if let Err(e) = session(&stream, &console) {
                eprintln!("ed-bridge: {e}");
            }
            // `current` keeps a clone open: close the socket whatever ended the session.
            let _ = stream.shutdown(Shutdown::Both);
        });
    }
}

fn session(stream: &TcpStream, console: &str) -> Result<(), String> {
    let to_console = TcpStream::connect(console).map_err(|e| format!("{console}: {e}"))?;
    let from_console = to_console.try_clone().map_err(|e| e.to_string())?;
    let (lines, rx) = mpsc::channel::<String>();
    // The console's lines; the thread ends when the console or the session does.
    std::thread::spawn(move || {
        for line in BufReader::new(from_console).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                return;
            }
        }
    });
    let stream = stream.try_clone().map_err(|e| e.to_string())?;
    let result = bridge(stream, &to_console, &rx);
    let _ = to_console.shutdown(Shutdown::Both);
    result
}

fn bridge(
    stream: TcpStream,
    mut console: &TcpStream,
    rx: &mpsc::Receiver<String>,
) -> Result<(), String> {
    // The handshake blocks (a slow client gets a while); only then poll.
    stream
        .set_read_timeout(Some(HANDSHAKE))
        .map_err(|e| e.to_string())?;
    let mut ws: WebSocket<TcpStream> = tungstenite::accept(stream).map_err(|e| e.to_string())?;
    ws.get_ref()
        .set_read_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                let line = t.as_str().trim_end_matches(['\r', '\n']);
                // One message, one command: an embedded newline is dropped.
                if line.contains(['\r', '\n']) {
                    continue;
                }
                let line = format!("{line}\n");
                console
                    .write_all(line.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return Ok(()),
        }
        loop {
            match rx.try_recv() {
                Ok(line) => ws.send(Message::text(line)).map_err(|e| e.to_string())?,
                Err(TryRecvError::Empty) => break,
                // The console is gone: close the socket.
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            }
        }
    }
}
