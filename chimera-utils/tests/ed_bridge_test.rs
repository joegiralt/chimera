//! ed-bridge against a fake TCP console: lines both ways, a gone console,
//! and a second browser replacing the first.

#[allow(dead_code)]
#[path = "../src/ed_bridge.rs"]
mod ed_bridge;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// A fake console and a bridge to it; returns the console and the bridge's URL.
fn rig() -> (TcpListener, String) {
    let console = TcpListener::bind("127.0.0.1:0").unwrap();
    let caddr = console.local_addr().unwrap().to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || ed_bridge::serve(listener, &caddr));
    (console, url)
}

fn browser(url: &str) -> Ws {
    let (ws, _) = tungstenite::connect(url).unwrap();
    if let MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    }
    ws
}

fn text(ws: &mut Ws) -> Option<String> {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return Some(t.as_str().to_owned()),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

#[test]
fn bridges_lines_both_ways() {
    let (console, url) = rig();
    let mut ws = browser(&url);
    let (c, _) = console.accept().unwrap();
    let mut cr = BufReader::new(c.try_clone().unwrap());
    let mut cw = c;
    ws.send(Message::text("ed hello")).unwrap();
    let mut line = String::new();
    cr.read_line(&mut line).unwrap();
    assert_eq!(line, "ed hello\n");
    cw.write_all(b"ed ok 1\r\nsecond\n").unwrap();
    assert_eq!(text(&mut ws).unwrap(), "ed ok 1");
    assert_eq!(text(&mut ws).unwrap(), "second");
}

#[test]
fn console_gone_closes_socket() {
    let (console, url) = rig();
    let mut ws = browser(&url);
    let (c, _) = console.accept().unwrap();
    drop(c);
    assert_eq!(text(&mut ws), None);
}

#[test]
fn second_client_replaces_first() {
    let (console, url) = rig();
    let mut first = browser(&url);
    let (c1, _) = console.accept().unwrap();
    let mut second = browser(&url);
    let (c2, _) = console.accept().unwrap();
    // The first is closed, and its console connection with it.
    assert_eq!(text(&mut first), None);
    let mut r1 = BufReader::new(c1);
    r1.get_ref()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut s = String::new();
    assert_eq!(r1.read_line(&mut s).unwrap(), 0);
    // The second works.
    second.send(Message::text("ed hello")).unwrap();
    let mut line = String::new();
    BufReader::new(c2).read_line(&mut line).unwrap();
    assert_eq!(line, "ed hello\n");
}
