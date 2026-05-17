//! A dependency-free RFC 6455 WebSocket server [`Transport`].
//!
//! The protocol is the same JSON-RPC 2.0 [`Message`] as stdio — only the
//! framing differs (one text frame per message). A plugin that wants to be
//! a long-lived / remote endpoint binds an address, the client connects,
//! and the SDK dispatch loop runs unchanged.
//!
//! Zero new crates (the workspace dependency budget + `cargo deny` are
//! strict): SHA-1 and base64 for the handshake are implemented inline, and
//! framing uses only `std::net`/`std::io` (plugins are synchronous).

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::jsonrpc::Message;
use crate::transport::Transport;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// A WebSocket server transport over one accepted TCP connection.
pub struct WsTransport {
    stream: TcpStream,
}

impl WsTransport {
    /// Binds `addr`, accepts the first client, performs the RFC 6455
    /// handshake, and returns the framed transport.
    ///
    /// # Errors
    ///
    /// Bind/accept/handshake I/O failures.
    pub fn accept(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let (stream, _) = listener.accept()?;
        Self::from_accepted(stream)
    }

    /// Runs the RFC 6455 handshake on an already-accepted TCP stream.
    ///
    /// # Errors
    ///
    /// Handshake I/O failures.
    pub fn from_accepted(mut stream: TcpStream) -> io::Result<Self> {
        handshake(&mut stream)?;
        Ok(Self { stream })
    }
}

impl Transport for WsTransport {
    fn recv(&mut self) -> io::Result<Option<Message>> {
        match read_text_message(&mut self.stream)? {
            None => Ok(None),
            Some(text) => {
                if text.trim().is_empty() {
                    // An empty frame is not a message; treat as keep-alive.
                    return self.recv();
                }
                Message::decode_line(&text)
                    .map(Some)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
        }
    }

    fn send(&mut self, msg: &Message) -> io::Result<()> {
        let mut payload = serde_json::to_string(msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        // Trim the trailing newline `encode_line` would add — WS framing
        // delimits messages, not newlines.
        if payload.ends_with('\n') {
            payload.pop();
        }
        write_text_frame(&mut self.stream, payload.as_bytes())
    }
}

// ---- handshake -------------------------------------------------------

fn handshake(stream: &mut TcpStream) -> io::Result<()> {
    // Read request headers up to the blank line.
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed during handshake",
            ));
        }
        buf.push(byte[0]);
        if buf.len() > 16 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "handshake headers too large",
            ));
        }
    }
    let req = String::from_utf8_lossy(&buf);
    let key = req
        .lines()
        .find_map(|l| {
            let (h, v) = l.split_once(':')?;
            h.trim()
                .eq_ignore_ascii_case("sec-websocket-key")
                .then(|| v.trim().to_owned())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no Sec-WebSocket-Key"))?;

    let accept = base64_encode(&sha1(format!("{key}{WS_GUID}").as_bytes()));
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}

// ---- framing ---------------------------------------------------------

fn read_exact(stream: &mut TcpStream, n: usize) -> io::Result<Option<Vec<u8>>> {
    let mut v = vec![0u8; n];
    let mut read = 0;
    while read < n {
        let k = stream.read(&mut v[read..])?;
        if k == 0 {
            return Ok(None); // EOF
        }
        read += k;
    }
    Ok(Some(v))
}

/// Reads frames until a full text message (handling continuation, ping,
/// pong, close, and 7/16/64-bit lengths). `Ok(None)` = connection closed.
fn read_text_message(stream: &mut TcpStream) -> io::Result<Option<String>> {
    let mut data: Vec<u8> = Vec::new();
    loop {
        let Some(hdr) = read_exact(stream, 2)? else {
            return Ok(None);
        };
        let fin = hdr[0] & 0x80 != 0;
        let opcode = hdr[0] & 0x0f;
        let masked = hdr[1] & 0x80 != 0;
        let mut len = u64::from(hdr[1] & 0x7f);
        if len == 126 {
            let Some(e) = read_exact(stream, 2)? else {
                return Ok(None);
            };
            len = u64::from(u16::from_be_bytes([e[0], e[1]]));
        } else if len == 127 {
            let Some(e) = read_exact(stream, 8)? else {
                return Ok(None);
            };
            len = u64::from_be_bytes(e.try_into().expect("8 bytes"));
        }
        let mask = if masked {
            match read_exact(stream, 4)? {
                Some(m) => Some(m),
                None => return Ok(None),
            }
        } else {
            None
        };
        let payload = if len == 0 {
            Vec::new()
        } else {
            let Some(mut p) = read_exact(stream, len as usize)? else {
                return Ok(None);
            };
            if let Some(m) = &mask {
                for (i, b) in p.iter_mut().enumerate() {
                    *b ^= m[i % 4];
                }
            }
            p
        };
        match opcode {
            0x8 => return Ok(None), // close
            0x9 => {
                write_frame(stream, 0xA, &payload)?; // ping → pong
                continue;
            }
            0xA => continue, // pong
            0x0..=0x2 => {
                data.extend_from_slice(&payload);
                if fin {
                    return Ok(Some(String::from_utf8_lossy(&data).into_owned()));
                }
            }
            _ => return Ok(None),
        }
    }
}

fn write_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut frame = vec![0x80 | opcode]; // FIN + opcode
    let n = payload.len();
    if n < 126 {
        frame.push(n as u8);
    } else if n <= u16::MAX as usize {
        frame.push(126);
        frame.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(n as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    stream.write_all(&frame)?;
    stream.flush()
}

fn write_text_frame(stream: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    write_frame(stream, 0x1, payload)
}

// ---- SHA-1 + base64 (handshake only; dependency-free) ----------------

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let ml = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64_encode, sha1};

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(base64_encode(&sha1(b"")), "2jmj7l5rSw0yVb/vlWAYkK/YBwk=",);
        // RFC 6455 §1.3 worked example.
        let accept = base64_encode(&sha1(
            b"dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
        ));
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn base64_padding_is_correct() {
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn websocket_handshake_and_message_round_trip() {
        use crate::jsonrpc::Message;
        use crate::transport::Transport;
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server: accept + handshake, recv one JSON-RPC msg, send one back.
        let server = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut t = super::WsTransport::from_accepted(s).unwrap();
            let got = t.recv().unwrap().expect("a message");
            assert_eq!(got.method.as_deref(), Some("tick"));
            t.send(&Message::notification(
                "ui/note",
                Some(serde_json::json!({ "type": "note", "text": "pong" })),
            ))
            .unwrap();
        });

        // Client: minimal RFC 6455 handshake + a masked text frame.
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(
            b"GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n\
              Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
              Sec-WebSocket-Version: 13\r\n\r\n",
        )
        .unwrap();
        let mut hdr = Vec::new();
        let mut b = [0u8; 1];
        while !hdr.ends_with(b"\r\n\r\n") {
            c.read_exact(&mut b).unwrap();
            hdr.push(b[0]);
        }
        assert!(
            String::from_utf8_lossy(&hdr)
                .contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            "RFC 6455 accept key",
        );

        let body = br#"{"jsonrpc":"2.0","method":"tick"}"#;
        let mut frame = vec![0x81u8, 0x80 | body.len() as u8];
        let mask = [0x01, 0x02, 0x03, 0x04];
        frame.extend_from_slice(&mask);
        frame.extend(body.iter().enumerate().map(|(i, x)| x ^ mask[i % 4]));
        c.write_all(&frame).unwrap();

        // Read the server's (unmasked) reply frame.
        let mut h2 = [0u8; 2];
        c.read_exact(&mut h2).unwrap();
        let len = (h2[1] & 0x7f) as usize;
        let mut pl = vec![0u8; len];
        c.read_exact(&mut pl).unwrap();
        let reply: Message = serde_json::from_slice(&pl).unwrap();
        assert_eq!(reply.method.as_deref(), Some("ui/note"));

        server.join().unwrap();
    }
}
