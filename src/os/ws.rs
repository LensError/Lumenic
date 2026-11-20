#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::os::Waker;
use crate::ui::types::Error;

/// Hardened frame, message, and handshake byte limits enforced by `WsClient`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WsLimits {
    /// Maximum compressed/uncompressed size of a single WebSocket frame in bytes.
    pub max_frame_bytes: usize,
    /// Maximum reassembled WebSocket message payload size in bytes.
    pub max_message_bytes: usize,
    /// Maximum HTTP upgrade response header size in bytes.
    pub max_handshake_bytes: usize,
}

impl Default for WsLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1024 * 1024,
            max_message_bytes: 4 * 1024 * 1024,
            max_handshake_bytes: 16 * 1024,
        }
    }
}

/// Incoming WebSocket lifecycle and payload events delivered to the UI queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsMessage {
    /// Connection handshake succeeded and socket is ready.
    Connected,
    /// Incoming UTF-8 text frame/message.
    Text(String),
    /// Incoming raw binary frame/message.
    Binary(Vec<u8>),
    /// Connection closed with optional close code and reason string.
    Closed(Option<u16>, String),
    /// Transport or protocol error message.
    Error(String),
}

#[cfg(any(test, all(not(target_arch = "wasm32"), feature = "network")))]
mod rfc6455 {
    use std::io::{self, Read};

    use super::WsMessage;
    use crate::ui::types::Error;

    const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    pub const OPCODE_CONT: u8 = 0x0;
    pub const OPCODE_TEXT: u8 = 0x1;
    pub const OPCODE_BINARY: u8 = 0x2;
    pub const OPCODE_CLOSE: u8 = 0x8;
    pub const OPCODE_PING: u8 = 0x9;
    pub const OPCODE_PONG: u8 = 0xA;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct RawFrame {
        pub fin: bool,
        pub opcode: u8,
        pub payload: Vec<u8>,
    }

    fn sha1_digest(input: &[u8]) -> [u8; 20] {
        let mut h0: u32 = 0x6745_2301;
        let mut h1: u32 = 0xEFCD_AB89;
        let mut h2: u32 = 0x98BA_DCFE;
        let mut h3: u32 = 0x1032_5476;
        let mut h4: u32 = 0xC3D2_E1F0;

        let bit_len = (input.len() as u64).saturating_mul(8);
        let mut msg = input.to_vec();
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        for block in msg.as_chunks::<64>().0 {
            let mut w = [0u32; 80];
            for (i, word_bytes) in block.as_chunks::<4>().0.iter().enumerate() {
                w[i] = u32::from_be_bytes(*word_bytes);
            }
            for i in 16..80 {
                w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
            }

            let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
            for (i, &wk) in w.iter().enumerate() {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999_u32),
                    20..=39 => (b ^ c ^ d, 0x6ED9_EBA1_u32),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC_u32),
                    _ => (b ^ c ^ d, 0xCA62_C1D6_u32),
                };
                let temp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(wk);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = temp;
            }

            h0 = h0.wrapping_add(a);
            h1 = h1.wrapping_add(b);
            h2 = h2.wrapping_add(c);
            h3 = h3.wrapping_add(d);
            h4 = h4.wrapping_add(e);
        }

        let mut out = [0u8; 20];
        for (dst, val) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip([h0, h1, h2, h3, h4])
        {
            *dst = val.to_be_bytes();
        }
        out
    }

    pub fn base64_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = u32::from(chunk[0]);
            let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
            let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    pub fn compute_accept_key(sec_key: &str) -> String {
        let mut buf = Vec::with_capacity(sec_key.len() + WS_GUID.len());
        buf.extend_from_slice(sec_key.trim().as_bytes());
        buf.extend_from_slice(WS_GUID);
        base64_encode(&sha1_digest(&buf))
    }

    pub fn encode_client_frame(
        opcode: u8,
        fin: bool,
        mask_key: [u8; 4],
        payload: &[u8],
    ) -> Result<Vec<u8>, Error> {
        if opcode >= 0x8 && payload.len() > 125 {
            return Err(Error::InvalidInput(String::from(
                "RFC 6455 control frame payload exceeds 125 bytes",
            )));
        }
        let header_len: usize = if payload.len() <= 125 {
            6
        } else if payload.len() <= u16::MAX as usize {
            8
        } else {
            14
        };
        let total_len = header_len
            .checked_add(payload.len())
            .ok_or_else(|| Error::InvalidInput(String::from("WebSocket frame length overflow")))?;

        let mut out = Vec::with_capacity(total_len);
        let b0 = if fin {
            0x80 | (opcode & 0x0F)
        } else {
            opcode & 0x0F
        };
        out.push(b0);

        if payload.len() <= 125 {
            out.push(0x80 | (payload.len() as u8));
        } else if payload.len() <= u16::MAX as usize {
            out.push(0x80 | 126);
            out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            out.push(0x80 | 127);
            out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }

        out.extend_from_slice(&mask_key);
        for (i, &byte) in payload.iter().enumerate() {
            out.push(byte ^ mask_key[i & 3]);
        }
        Ok(out)
    }

    fn read_exact_retrying<R: Read>(reader: &mut R, mut dst: &mut [u8]) -> Result<(), Error> {
        while !dst.is_empty() {
            match reader.read(dst) {
                Ok(0) => return Err(Error::Io(String::from("WebSocket unexpected EOF"))),
                Ok(n) => dst = &mut dst[n..],
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(Error::Io(format!("WebSocket read error: {e}"))),
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn read_frame<R: Read>(reader: &mut R, max_frame_bytes: usize) -> Result<RawFrame, Error> {
        let mut b0 = [0u8; 1];
        read_exact_retrying(reader, &mut b0)?;
        read_frame_after_b0(b0[0], reader, max_frame_bytes)
    }

    pub fn read_frame_after_b0<R: Read>(
        b0: u8,
        reader: &mut R,
        max_frame_bytes: usize,
    ) -> Result<RawFrame, Error> {
        if (b0 & 0x70) != 0 {
            return Err(Error::InvalidInput(String::from(
                "RFC 6455 non-zero RSV bits are unsupported",
            )));
        }
        let fin = (b0 & 0x80) != 0;
        let opcode = b0 & 0x0F;

        let mut b1 = [0u8; 1];
        read_exact_retrying(reader, &mut b1)?;
        let masked = (b1[0] & 0x80) != 0;
        let len_code = b1[0] & 0x7F;

        let payload_len: usize = match len_code {
            0..=125 => usize::from(len_code),
            126 => {
                let mut ext = [0u8; 2];
                read_exact_retrying(reader, &mut ext)?;
                usize::from(u16::from_be_bytes(ext))
            }
            _ => {
                let mut ext = [0u8; 8];
                read_exact_retrying(reader, &mut ext)?;
                let raw_u64 = u64::from_be_bytes(ext);
                usize::try_from(raw_u64).map_err(|_| {
                    Error::InvalidInput(String::from("WebSocket 64-bit frame length overflow"))
                })?
            }
        };

        if opcode >= 0x8 {
            if !fin {
                return Err(Error::InvalidInput(String::from(
                    "RFC 6455 control frames must not be fragmented",
                )));
            }
            if payload_len > 125 {
                return Err(Error::InvalidInput(String::from(
                    "RFC 6455 control frame exceeds 125 bytes",
                )));
            }
        }

        if payload_len > max_frame_bytes {
            return Err(Error::InvalidInput(format!(
                "WebSocket frame size {payload_len} exceeds limit {max_frame_bytes}"
            )));
        }

        let mask_key = if masked {
            let mut mk = [0u8; 4];
            read_exact_retrying(reader, &mut mk)?;
            Some(mk)
        } else {
            None
        };

        let mut payload = vec![0u8; payload_len];
        read_exact_retrying(reader, &mut payload)?;
        if let Some(mk) = mask_key {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mk[i & 3];
            }
        }

        Ok(RawFrame {
            fin,
            opcode,
            payload,
        })
    }

    pub struct WsAssembler {
        max_message_bytes: usize,
        active_opcode: Option<u8>,
        frag_buf: Vec<u8>,
    }

    pub struct AssembleOutcome {
        pub message: Option<WsMessage>,
        pub reply_frame: Option<(u8, Vec<u8>)>,
        pub should_close: bool,
    }

    impl WsAssembler {
        pub fn new(max_message_bytes: usize) -> Self {
            Self {
                max_message_bytes,
                active_opcode: None,
                frag_buf: Vec::new(),
            }
        }

        pub fn push_frame(&mut self, frame: RawFrame) -> Result<AssembleOutcome, Error> {
            match frame.opcode {
                OPCODE_PING => Ok(AssembleOutcome {
                    message: None,
                    reply_frame: Some((OPCODE_PONG, frame.payload)),
                    should_close: false,
                }),
                OPCODE_PONG => Ok(AssembleOutcome {
                    message: None,
                    reply_frame: None,
                    should_close: false,
                }),
                OPCODE_CLOSE => {
                    let code = if frame.payload.len() >= 2 {
                        Some(u16::from_be_bytes([frame.payload[0], frame.payload[1]]))
                    } else {
                        None
                    };
                    let reason = if frame.payload.len() > 2 {
                        String::from_utf8_lossy(&frame.payload[2..]).into_owned()
                    } else {
                        String::new()
                    };
                    Ok(AssembleOutcome {
                        message: Some(WsMessage::Closed(code, reason)),
                        reply_frame: Some((OPCODE_CLOSE, frame.payload)),
                        should_close: true,
                    })
                }
                OPCODE_TEXT | OPCODE_BINARY => {
                    if self.active_opcode.is_some() {
                        return Err(Error::InvalidInput(String::from(
                            "unexpected new data frame before previous fragmented message finished",
                        )));
                    }
                    if frame.payload.len() > self.max_message_bytes {
                        return Err(Error::InvalidInput(String::from(
                            "WebSocket message exceeds max_message_bytes",
                        )));
                    }
                    if frame.fin {
                        let msg = decode_completed_message(frame.opcode, frame.payload)?;
                        return Ok(AssembleOutcome {
                            message: Some(msg),
                            reply_frame: None,
                            should_close: false,
                        });
                    }
                    self.active_opcode = Some(frame.opcode);
                    self.frag_buf.clear();
                    self.frag_buf.extend_from_slice(&frame.payload);
                    Ok(AssembleOutcome {
                        message: None,
                        reply_frame: None,
                        should_close: false,
                    })
                }
                OPCODE_CONT => {
                    let Some(op) = self.active_opcode else {
                        return Err(Error::InvalidInput(String::from(
                            "unexpected WebSocket continuation frame",
                        )));
                    };
                    let next_len = self
                        .frag_buf
                        .len()
                        .checked_add(frame.payload.len())
                        .ok_or_else(|| {
                            Error::InvalidInput(String::from(
                                "WebSocket fragmented length overflow",
                            ))
                        })?;
                    if next_len > self.max_message_bytes {
                        return Err(Error::InvalidInput(String::from(
                            "fragmented WebSocket message exceeds max_message_bytes",
                        )));
                    }
                    self.frag_buf.extend_from_slice(&frame.payload);
                    if frame.fin {
                        self.active_opcode = None;
                        let bytes = core::mem::take(&mut self.frag_buf);
                        let msg = decode_completed_message(op, bytes)?;
                        return Ok(AssembleOutcome {
                            message: Some(msg),
                            reply_frame: None,
                            should_close: false,
                        });
                    }
                    Ok(AssembleOutcome {
                        message: None,
                        reply_frame: None,
                        should_close: false,
                    })
                }
                other => Err(Error::InvalidInput(format!(
                    "unsupported WebSocket opcode {other}"
                ))),
            }
        }
    }

    fn decode_completed_message(opcode: u8, bytes: Vec<u8>) -> Result<WsMessage, Error> {
        if opcode == OPCODE_TEXT {
            let s = String::from_utf8(bytes)
                .map_err(|e| Error::InvalidInput(format!("invalid UTF-8 WebSocket text: {e}")))?;
            Ok(WsMessage::Text(s))
        } else {
            Ok(WsMessage::Binary(bytes))
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
mod native_transport {
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use super::rfc6455::{
        base64_encode, compute_accept_key, encode_client_frame, read_frame_after_b0, WsAssembler,
        OPCODE_BINARY, OPCODE_CLOSE, OPCODE_TEXT,
    };
    use super::{WsLimits, WsMessage};
    use crate::os::Waker;
    use crate::ui::types::Error;

    struct ParsedWsUrl {
        tls: bool,
        host: String,
        port: u16,
        path_and_query: String,
    }

    fn parse_ws_url(url: &str) -> Result<ParsedWsUrl, Error> {
        let (tls, rest) = if let Some(r) = url.strip_prefix("wss://") {
            (true, r)
        } else if let Some(r) = url.strip_prefix("ws://") {
            (false, r)
        } else {
            return Err(Error::InvalidInput(String::from(
                "WebSocket URL must begin with ws:// or wss://",
            )));
        };

        let (authority, path_and_query) = match rest.find('/') {
            Some(idx) => (&rest[..idx], rest[idx..].to_owned()),
            None => (rest, String::from("/")),
        };
        if authority.is_empty() {
            return Err(Error::InvalidInput(String::from(
                "WebSocket URL missing host",
            )));
        }

        let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
            let parsed_port = p
                .parse::<u16>()
                .map_err(|_| Error::InvalidInput(String::from("invalid WebSocket port")))?;
            (h.to_owned(), parsed_port)
        } else {
            (authority.to_owned(), if tls { 443 } else { 80 })
        };

        Ok(ParsedWsUrl {
            tls,
            host,
            port,
            path_and_query,
        })
    }

    enum NativeStream {
        Plain(TcpStream),
        Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
    }

    impl Read for NativeStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.read(buf),
                Self::Tls(s) => s.read(buf),
            }
        }
    }

    impl Write for NativeStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.write(buf),
                Self::Tls(s) => s.write(buf),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            match self {
                Self::Plain(s) => s.flush(),
                Self::Tls(s) => s.flush(),
            }
        }
    }

    fn next_mask_key() -> [u8; 4] {
        static SEED: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
        let x = SEED.fetch_add(0xD134_2543_DE82_EF95, Ordering::Relaxed);
        let mixed = x ^ (x >> 30);
        [
            (mixed & 0xFF) as u8,
            ((mixed >> 8) & 0xFF) as u8,
            ((mixed >> 16) & 0xFF) as u8,
            ((mixed >> 24) & 0xFF) as u8,
        ]
    }

    fn push_event(inbox: &Arc<Mutex<Vec<WsMessage>>>, msg: WsMessage, waker: &Waker) {
        if let Ok(mut guard) = inbox.lock() {
            guard.push(msg);
        }
        waker.wake();
    }

    fn perform_handshake(
        stream: &mut NativeStream,
        parsed: &ParsedWsUrl,
        max_handshake_bytes: usize,
    ) -> Result<(), Error> {
        let m = next_mask_key();
        let raw_nonce = [
            m[0], m[1], m[2], m[3], 0x4C, 0x75, 0x6D, 0x65, 0x6E, 0x69, 0x63, 0x31, 0x32, 0x33,
            0x34, 0x35,
        ];
        let sec_key = base64_encode(&raw_nonce);
        let expected_accept = compute_accept_key(&sec_key);

        let host_header =
            if (parsed.tls && parsed.port == 443) || (!parsed.tls && parsed.port == 80) {
                parsed.host.clone()
            } else {
                format!("{}:{}", parsed.host, parsed.port)
            };

        let req = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            parsed.path_and_query, host_header, sec_key
        );
        stream
            .write_all(req.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|e| Error::Io(format!("WebSocket handshake write failed: {e}")))?;

        let mut reader = BufReader::new(stream);
        let mut total = 0usize;
        let mut status_ok = false;
        let mut accept_ok = false;

        loop {
            let mut chunk = Vec::new();
            loop {
                let avail = reader
                    .fill_buf()
                    .map_err(|e| Error::Io(format!("WebSocket handshake read failed: {e}")))?;
                if avail.is_empty() {
                    return Err(Error::Io(String::from("WebSocket handshake EOF")));
                }
                if let Some(pos) = avail.iter().position(|&b| b == b'\n') {
                    let take = pos + 1;
                    total = total.checked_add(take).ok_or_else(|| {
                        Error::InvalidInput(String::from("handshake size overflow"))
                    })?;
                    if total > max_handshake_bytes {
                        return Err(Error::InvalidInput(String::from(
                            "WebSocket handshake exceeded limit",
                        )));
                    }
                    chunk.extend_from_slice(&avail[..take]);
                    reader.consume(take);
                    break;
                }
                let take = avail.len();
                total = total
                    .checked_add(take)
                    .ok_or_else(|| Error::InvalidInput(String::from("handshake size overflow")))?;
                if total > max_handshake_bytes {
                    return Err(Error::InvalidInput(String::from(
                        "WebSocket handshake exceeded limit",
                    )));
                }
                chunk.extend_from_slice(avail);
                reader.consume(take);
            }

            let trimmed = String::from_utf8_lossy(&chunk).trim_end().to_owned();
            if trimmed.is_empty() {
                break;
            }
            if !status_ok {
                if !trimmed.contains(" 101 ") {
                    return Err(Error::Io(format!(
                        "unexpected WebSocket HTTP status: {trimmed}"
                    )));
                }
                status_ok = true;
            } else if let Some((name, val)) = trimmed.split_once(':') {
                if name.trim().eq_ignore_ascii_case("sec-websocket-accept")
                    && val.trim() == expected_accept
                {
                    accept_ok = true;
                }
            }
        }

        if !status_ok || !accept_ok {
            return Err(Error::Io(String::from(
                "invalid Sec-WebSocket-Accept header in server response",
            )));
        }
        Ok(())
    }

    type OutboxQueue = Arc<Mutex<Vec<(u8, Vec<u8>)>>>;

    pub struct NativeWsHandle {
        outbox: OutboxQueue,
        closed: Arc<AtomicBool>,
    }

    pub fn spawn_native_ws(
        url: &str,
        limits: WsLimits,
        inbox: Arc<Mutex<Vec<WsMessage>>>,
        waker: Waker,
    ) -> Result<NativeWsHandle, Error> {
        let parsed = parse_ws_url(url)?;
        let outbox = Arc::new(Mutex::new(Vec::<(u8, Vec<u8>)>::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let outbox_worker = Arc::clone(&outbox);
        let closed_worker = Arc::clone(&closed);

        thread::spawn(move || {
            let addr = format!("{}:{}", parsed.host, parsed.port);
            let tcp = match TcpStream::connect(&addr) {
                Ok(s) => s,
                Err(e) => {
                    push_event(
                        &inbox,
                        WsMessage::Error(format!("connect failed: {e}")),
                        &waker,
                    );
                    return;
                }
            };
            let _ = tcp.set_read_timeout(Some(Duration::from_millis(20)));
            let _ = tcp.set_write_timeout(Some(Duration::from_secs(5)));

            let mut stream = if parsed.tls {
                let mut roots = rustls::RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let cfg = rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let Ok(sni) = rustls::pki_types::ServerName::try_from(parsed.host.clone()) else {
                    push_event(
                        &inbox,
                        WsMessage::Error(String::from("invalid TLS SNI server name")),
                        &waker,
                    );
                    return;
                };
                let Ok(conn) = rustls::ClientConnection::new(Arc::new(cfg), sni) else {
                    push_event(
                        &inbox,
                        WsMessage::Error(String::from("TLS client init failed")),
                        &waker,
                    );
                    return;
                };
                NativeStream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp)))
            } else {
                NativeStream::Plain(tcp)
            };

            if let Err(e) = perform_handshake(&mut stream, &parsed, limits.max_handshake_bytes) {
                push_event(&inbox, WsMessage::Error(e.to_string()), &waker);
                return;
            }
            push_event(&inbox, WsMessage::Connected, &waker);

            let mut assembler = WsAssembler::new(limits.max_message_bytes);
            loop {
                let pending: Vec<(u8, Vec<u8>)> = outbox_worker
                    .lock()
                    .map(|mut g| core::mem::take(&mut *g))
                    .unwrap_or_default();
                for (op, data) in pending {
                    if let Ok(encoded) = encode_client_frame(op, true, next_mask_key(), &data) {
                        if stream
                            .write_all(&encoded)
                            .and_then(|_| stream.flush())
                            .is_err()
                        {
                            closed_worker.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    if op == OPCODE_CLOSE {
                        closed_worker.store(true, Ordering::SeqCst);
                    }
                }
                if closed_worker.load(Ordering::SeqCst) {
                    push_event(&inbox, WsMessage::Closed(Some(1000), String::new()), &waker);
                    break;
                }

                let mut b0 = [0u8; 1];
                match stream.read(&mut b0) {
                    Ok(0) => {
                        push_event(&inbox, WsMessage::Closed(None, String::new()), &waker);
                        break;
                    }
                    Ok(_) => {
                        match read_frame_after_b0(b0[0], &mut stream, limits.max_frame_bytes) {
                            Ok(frame) => match assembler.push_frame(frame) {
                                Ok(outcome) => {
                                    if let Some((rop, rpay)) = outcome.reply_frame {
                                        if let Ok(enc) =
                                            encode_client_frame(rop, true, next_mask_key(), &rpay)
                                        {
                                            let _ =
                                                stream.write_all(&enc).and_then(|_| stream.flush());
                                        }
                                    }
                                    if let Some(msg) = outcome.message {
                                        push_event(&inbox, msg, &waker);
                                    }
                                    if outcome.should_close {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    push_event(&inbox, WsMessage::Error(e.to_string()), &waker);
                                    break;
                                }
                            },
                            Err(e) => {
                                push_event(&inbox, WsMessage::Error(e.to_string()), &waker);
                                break;
                            }
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock
                                | io::ErrorKind::TimedOut
                                | io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue;
                    }
                    Err(e) => {
                        push_event(
                            &inbox,
                            WsMessage::Error(format!("WebSocket I/O error: {e}")),
                            &waker,
                        );
                        break;
                    }
                }
            }
        });

        Ok(NativeWsHandle { outbox, closed })
    }

    pub fn enqueue_send(handle: &NativeWsHandle, op: u8, data: Vec<u8>) -> Result<(), Error> {
        if handle.closed.load(Ordering::SeqCst) {
            return Err(Error::Io(String::from("WebSocket is closed")));
        }
        let mut guard = handle
            .outbox
            .lock()
            .map_err(|_| Error::Io(String::from("WebSocket outbox lock poisoned")))?;
        guard.push((op, data));
        Ok(())
    }

    pub fn send_text(handle: &NativeWsHandle, text: &str) -> Result<(), Error> {
        enqueue_send(handle, OPCODE_TEXT, text.as_bytes().to_vec())
    }

    pub fn send_binary(handle: &NativeWsHandle, data: &[u8]) -> Result<(), Error> {
        enqueue_send(handle, OPCODE_BINARY, data.to_vec())
    }

    pub fn close(handle: &NativeWsHandle) -> Result<(), Error> {
        enqueue_send(handle, OPCODE_CLOSE, vec![0x03, 0xE8])
    }
}

#[cfg(all(target_arch = "wasm32", feature = "network"))]
mod wasm_transport {
    use std::sync::{Arc, Mutex};

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

    use super::WsMessage;
    use crate::os::Waker;
    use crate::ui::types::Error;

    pub struct WasmWsHandle {
        ws: WebSocket,
        _on_open: Closure<dyn FnMut()>,
        _on_msg: Closure<dyn FnMut(MessageEvent)>,
        _on_err: Closure<dyn FnMut(ErrorEvent)>,
        _on_close: Closure<dyn FnMut(CloseEvent)>,
    }

    pub fn connect_wasm(
        url: &str,
        inbox: Arc<Mutex<Vec<WsMessage>>>,
        waker: Waker,
    ) -> Result<WasmWsHandle, Error> {
        let ws = WebSocket::new(url)
            .map_err(|_| Error::Io(String::from("browser WebSocket::new failed")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let push = |q: &Arc<Mutex<Vec<WsMessage>>>, m: WsMessage, w: &Waker| {
            if let Ok(mut g) = q.lock() {
                g.push(m);
            }
            w.wake();
        };

        let q_open = Arc::clone(&inbox);
        let w_open = waker.clone();
        let on_open = Closure::wrap(Box::new(move || {
            push(&q_open, WsMessage::Connected, &w_open);
        }) as Box<dyn FnMut()>);

        let q_msg = Arc::clone(&inbox);
        let w_msg = waker.clone();
        let on_msg = Closure::wrap(Box::new(move |ev: MessageEvent| {
            if let Some(txt) = ev.data().as_string() {
                push(&q_msg, WsMessage::Text(txt), &w_msg);
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let q_err = Arc::clone(&inbox);
        let w_err = waker.clone();
        let on_err = Closure::wrap(Box::new(move |ev: ErrorEvent| {
            push(&q_err, WsMessage::Error(ev.message()), &w_err);
        }) as Box<dyn FnMut(ErrorEvent)>);

        let q_close = Arc::clone(&inbox);
        let w_close = waker;
        let on_close = Closure::wrap(Box::new(move |ev: CloseEvent| {
            push(
                &q_close,
                WsMessage::Closed(Some(ev.code()), ev.reason()),
                &w_close,
            );
        }) as Box<dyn FnMut(CloseEvent)>);

        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        Ok(WasmWsHandle {
            ws,
            _on_open: on_open,
            _on_msg: on_msg,
            _on_err: on_err,
            _on_close: on_close,
        })
    }

    pub fn send_text(h: &WasmWsHandle, text: &str) -> Result<(), Error> {
        h.ws.send_with_str(text)
            .map_err(|_| Error::Io(String::from("WebSocket text send failed")))
    }

    pub fn send_binary(h: &WasmWsHandle, data: &[u8]) -> Result<(), Error> {
        h.ws.send_with_u8_array(data)
            .map_err(|_| Error::Io(String::from("WebSocket binary send failed")))
    }

    pub fn close(h: &WasmWsHandle) -> Result<(), Error> {
        h.ws.close()
            .map_err(|_| Error::Io(String::from("WebSocket close failed")))
    }
}

/// Unified cross-platform WebSocket client (`ws://`/`wss://` via RFC 6455 + `rustls` on native; `web_sys::WebSocket` on wasm32).
pub struct WsClient {
    inbox: Arc<Mutex<Vec<WsMessage>>>,
    drain_buf: Mutex<VecDeque<WsMessage>>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
    handle: native_transport::NativeWsHandle,
    #[cfg(all(target_arch = "wasm32", feature = "network"))]
    handle: wasm_transport::WasmWsHandle,
}

impl WsClient {
    /// Connects to a WebSocket endpoint with default frame/message size caps and UI wakeup.
    pub fn connect(url: &str, waker: Waker) -> Result<Self, Error> {
        Self::connect_with_limits(url, WsLimits::default(), waker)
    }

    /// Connects to a WebSocket endpoint enforcing explicit `WsLimits`.
    pub fn connect_with_limits(url: &str, limits: WsLimits, waker: Waker) -> Result<Self, Error> {
        let inbox = Arc::new(Mutex::new(Vec::<WsMessage>::new()));
        #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
        {
            let handle = native_transport::spawn_native_ws(url, limits, Arc::clone(&inbox), waker)?;
            Ok(Self {
                inbox,
                drain_buf: Mutex::new(VecDeque::new()),
                handle,
            })
        }
        #[cfg(all(target_arch = "wasm32", feature = "network"))]
        {
            let _ = limits;
            let handle = wasm_transport::connect_wasm(url, Arc::clone(&inbox), waker)?;
            Ok(Self {
                inbox,
                drain_buf: Mutex::new(VecDeque::new()),
                handle,
            })
        }
        #[cfg(not(feature = "network"))]
        {
            let _ = (url, limits, waker, inbox);
            Err(Error::Io(String::from("network feature is disabled")))
        }
    }

    /// Queues a UTF-8 text message on the socket.
    pub fn send_text(&self, text: &str) -> Result<(), Error> {
        #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
        {
            native_transport::send_text(&self.handle, text)
        }
        #[cfg(all(target_arch = "wasm32", feature = "network"))]
        {
            wasm_transport::send_text(&self.handle, text)
        }
        #[cfg(not(feature = "network"))]
        {
            let _ = text;
            Err(Error::Io(String::from("network feature is disabled")))
        }
    }

    /// Queues a binary payload on the socket.
    pub fn send_binary(&self, data: &[u8]) -> Result<(), Error> {
        #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
        {
            native_transport::send_binary(&self.handle, data)
        }
        #[cfg(all(target_arch = "wasm32", feature = "network"))]
        {
            wasm_transport::send_binary(&self.handle, data)
        }
        #[cfg(not(feature = "network"))]
        {
            let _ = data;
            Err(Error::Io(String::from("network feature is disabled")))
        }
    }

    /// Sends a normal closure (`1000`) frame cleanly.
    pub fn close(&self) -> Result<(), Error> {
        #[cfg(all(not(target_arch = "wasm32"), feature = "network"))]
        {
            native_transport::close(&self.handle)
        }
        #[cfg(all(target_arch = "wasm32", feature = "network"))]
        {
            wasm_transport::close(&self.handle)
        }
        #[cfg(not(feature = "network"))]
        {
            Err(Error::Io(String::from("network feature is disabled")))
        }
    }

    fn refill_local_queue(&self) {
        let incoming = if let Ok(mut g) = self.inbox.lock() {
            core::mem::take(&mut *g)
        } else {
            Vec::new()
        };
        if !incoming.is_empty() {
            if let Ok(mut q) = self.drain_buf.lock() {
                q.extend(incoming);
            }
        }
    }

    /// Pops the next queued WebSocket event without blocking.
    pub fn poll(&self) -> Option<WsMessage> {
        self.refill_local_queue();
        self.drain_buf.lock().ok().and_then(|mut q| q.pop_front())
    }

    /// Drains all currently queued WebSocket events.
    pub fn drain(&self) -> Vec<WsMessage> {
        self.refill_local_queue();
        self.drain_buf
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::rfc6455::{
        compute_accept_key, encode_client_frame, read_frame, WsAssembler, OPCODE_CLOSE,
        OPCODE_CONT, OPCODE_PING, OPCODE_PONG, OPCODE_TEXT,
    };
    use super::WsMessage;

    struct ChunkedReader<'a> {
        data: &'a [u8],
        pos: usize,
        max_chunk: usize,
    }

    impl<'a> Read for ChunkedReader<'a> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let avail = (self.data.len() - self.pos)
                .min(self.max_chunk)
                .min(buf.len());
            buf[..avail].copy_from_slice(&self.data[self.pos..self.pos + avail]);
            self.pos += avail;
            Ok(avail)
        }
    }

    fn build_unmasked_server_frame(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(if fin { 0x80 | opcode } else { opcode });
        if payload.len() <= 125 {
            out.push(payload.len() as u8);
        } else {
            out.push(126);
            out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn rfc6455_masking_and_accept_key_vector() {
        assert_eq!(
            compute_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );

        let mask = [0x37, 0xFA, 0x21, 0x3D];
        let encoded =
            encode_client_frame(OPCODE_TEXT, true, mask, b"Hello, Lumenic!").expect("encode");
        assert_eq!(encoded[0], 0x81);
        assert_eq!(encoded[1] & 0x80, 0x80);
        let decoded = read_frame(&mut io::Cursor::new(encoded), 1024).expect("decode masked");
        assert!(decoded.fin);
        assert_eq!(decoded.opcode, OPCODE_TEXT);
        assert_eq!(decoded.payload, b"Hello, Lumenic!");
    }

    #[test]
    fn fragmented_tcp_stream_interleaved_ping_and_close() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&build_unmasked_server_frame(OPCODE_TEXT, false, b"Hello, "));
        wire.extend_from_slice(&build_unmasked_server_frame(OPCODE_PING, true, b"hb"));
        wire.extend_from_slice(&build_unmasked_server_frame(OPCODE_CONT, true, b"world!"));
        wire.extend_from_slice(&build_unmasked_server_frame(
            OPCODE_CLOSE,
            true,
            &[0x03, 0xE8, b'b', b'y', b'e'],
        ));

        let mut reader = ChunkedReader {
            data: &wire,
            pos: 0,
            max_chunk: 3,
        };
        let mut assembler = WsAssembler::new(4096);
        let mut events = Vec::new();
        let mut pong_seen = false;

        while let Ok(frame) = read_frame(&mut reader, 4096) {
            let outcome = assembler.push_frame(frame).expect("assemble");
            if let Some((OPCODE_PONG, pay)) = outcome.reply_frame {
                assert_eq!(pay, b"hb");
                pong_seen = true;
            }
            if let Some(m) = outcome.message {
                events.push(m);
            }
            if outcome.should_close {
                break;
            }
        }

        assert!(pong_seen);
        assert_eq!(
            events,
            vec![
                WsMessage::Text(String::from("Hello, world!")),
                WsMessage::Closed(Some(1000), String::from("bye")),
            ]
        );
    }

    #[test]
    fn oversized_frame_and_invalid_control_frame_rejected() {
        let big = build_unmasked_server_frame(OPCODE_TEXT, true, &[b'x'; 200]);
        assert!(read_frame(&mut io::Cursor::new(big), 128).is_err());

        let frag_ping = build_unmasked_server_frame(OPCODE_PING, false, b"no");
        assert!(read_frame(&mut io::Cursor::new(frag_ping), 4096).is_err());

        let long_ping = build_unmasked_server_frame(OPCODE_PING, true, &[0u8; 126]);
        assert!(read_frame(&mut io::Cursor::new(long_ping), 4096).is_err());
    }
}
