// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ed-bridge against a fake TCP console: lines both ways, a gone console,
//! and a second browser replacing the first.

#[allow(dead_code)]
#[path = "../src/ed_bridge.rs"]
mod ed_bridge;

use std::io::{BufRead, BufReader, ErrorKind, Write};
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

#[derive(Debug, PartialEq)]
enum Got {
    Text(String),
    /// A Close frame or the connection's end.
    Closed,
    /// Nothing within the read timeout.
    Timeout,
}

fn text(ws: &mut Ws) -> Got {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return Got::Text(t.as_str().to_owned()),
            Ok(Message::Close(_))
            | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Got::Closed;
            }
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                return Got::Timeout;
            }
            // A reset is the connection's end too.
            Err(tungstenite::Error::Io(_)) => return Got::Closed,
            Err(tungstenite::Error::Protocol(
                tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
            )) => return Got::Closed,
            Err(e) => panic!("{e}"),
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
    assert_eq!(text(&mut ws), Got::Text("ed ok 1".into()));
    assert_eq!(text(&mut ws), Got::Text("second".into()));
}

#[test]
fn console_gone_closes_socket() {
    let (console, url) = rig();
    let mut ws = browser(&url);
    let (c, _) = console.accept().unwrap();
    drop(c);
    assert_eq!(text(&mut ws), Got::Closed);
}

#[test]
fn second_client_replaces_first() {
    let (console, url) = rig();
    let mut first = browser(&url);
    let (c1, _) = console.accept().unwrap();
    let mut second = browser(&url);
    let (c2, _) = console.accept().unwrap();
    // The first is closed, and its console connection with it.
    assert_eq!(text(&mut first), Got::Closed);
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

#[test]
fn handshake_may_arrive_in_pieces() {
    let (console, url) = rig();
    let mut raw = TcpStream::connect(url.trim_start_matches("ws://")).unwrap();
    let req = "GET / HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
    let (a, b) = req.split_at(30);
    raw.write_all(a.as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    raw.write_all(b.as_bytes()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut head = String::new();
    BufReader::new(raw).read_line(&mut head).unwrap();
    assert!(head.contains("101"), "{head}");
    console.accept().unwrap();
}

#[test]
fn no_console_closes_the_browser_promptly() {
    // A port nothing listens on.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let caddr = dead.local_addr().unwrap().to_string();
    drop(dead);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || ed_bridge::serve(listener, &caddr));
    let t = std::time::Instant::now();
    // The upgrade is never answered: the connection closes under it.
    assert!(tungstenite::connect(&url).is_err());
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_message_is_one_line() {
    let (console, url) = rig();
    let mut ws = browser(&url);
    let (c, _) = console.accept().unwrap();
    ws.send(Message::text("ed set a\ned set b")).unwrap();
    ws.send(Message::text("ed hello")).unwrap();
    let mut line = String::new();
    BufReader::new(c).read_line(&mut line).unwrap();
    assert_eq!(
        line, "ed hello\n",
        "the two-line message never reached the console"
    );
}
