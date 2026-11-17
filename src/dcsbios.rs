//! DCS-BIOS connection: keeps a copy of the exported cockpit memory and sends commands back.
//!
//! The old ufc_writer / pfp_writer each opened their own TCP connection to 127.0.0.1:7778 and
//! assumed every read started on a frame boundary. Here one shared connection feeds a proper
//! byte-by-byte parser, reconnects on its own, and also listens to the standard UDP multicast
//! export (239.255.50.10:5010) so either DCS-BIOS transport works.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::log::log;

pub type Snapshot = HashMap<u16, [u8; 2]>;

const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 50, 10);
const MULTICAST_PORT: u16 = 5010;
const BUTTON_SLEEP_DELAY: Duration = Duration::from_millis(100);

pub struct DcsBios {
    mem: Mutex<Snapshot>,
    last_data: Mutex<Option<(Instant, &'static str)>>,
    tcp_out: Mutex<Option<TcpStream>>,
    host: Mutex<String>,
    port: Mutex<u16>,
}

static BIOS: OnceLock<DcsBios> = OnceLock::new();

pub fn bios() -> &'static DcsBios {
    BIOS.get_or_init(|| DcsBios {
        mem: Mutex::new(HashMap::new()),
        last_data: Mutex::new(None),
        tcp_out: Mutex::new(None),
        host: Mutex::new("127.0.0.1".into()),
        port: Mutex::new(7778),
    })
}

/// Start the background readers (TCP client and UDP multicast listener). Call once.
pub fn start(host: &str, port: u16) {
    let b = bios();
    *b.host.lock().unwrap() = host.to_string();
    *b.port.lock().unwrap() = port;
    thread::Builder::new().name("dcsbios-tcp".into()).spawn(tcp_loop).ok();
    thread::Builder::new().name("dcsbios-udp".into()).spawn(udp_loop).ok();
}

impl DcsBios {
    pub fn snapshot(&self) -> Snapshot {
        match self.mem.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    /// Some(transport) if data arrived within the last two seconds.
    pub fn connected(&self) -> Option<&'static str> {
        let g = self.last_data.lock().unwrap();
        g.and_then(|(t, via)| (t.elapsed() < Duration::from_secs(2)).then_some(via))
    }

    pub fn module_name(&self) -> String {
        let snap = self.snapshot();
        module_name(&snap)
    }

    fn mark(&self, via: &'static str) {
        let mut g = self.last_data.lock().unwrap();
        let was = g.map(|(t, _)| t.elapsed() < Duration::from_secs(2)).unwrap_or(false);
        *g = Some((Instant::now(), via));
        if !was {
            log(format!("DCS-BIOS: Connection OK ({via})"));
        }
    }
}

pub fn module_name(snap: &Snapshot) -> String {
    get_string_by_addr_and_len(snap, 0x0000, 24).trim_end_matches(['?', ' ', '\0']).to_string()
}

pub fn get_value_by_address(values: &Snapshot, address: u16) -> [u8; 2] {
    values.get(&address).copied().unwrap_or([0, 0])
}

pub fn map_byte_to_char(b: u8) -> char {
    match b {
        0xB0 => '.',
        0xA1 => 'x',
        0xAB => 'y',
        0xBB => 'z',
        0xA9 => 'u',
        0xAE => '^',
        0xB6 => '_',
        0xB1 => '~',
        0x20..=0x7E => b.to_ascii_uppercase() as char,
        _ => '?',
    }
}

/// `length` is in bytes; every DCS-BIOS cell holds two.
pub fn get_string_by_addr_and_len(values: &Snapshot, base_addr: u16, length: u16) -> String {
    let cells = (length / 2) as usize;
    let mut out = String::with_capacity(length as usize);
    for i in 0..cells {
        let addr = base_addr.wrapping_add((2 * i) as u16);
        let [b0, b1] = get_value_by_address(values, addr);
        out.push(map_byte_to_char(b0));
        out.push(map_byte_to_char(b1));
    }
    out
}

// ------------------------------------------------------------------ parser

/// DCS-BIOS export stream parser (same state machine as the DCS-BIOS Arduino library).
#[derive(Default)]
pub struct Parser {
    state: u8,
    sync: u8,
    addr: u16,
    count: u16,
    lo: u8,
}

impl Parser {
    pub fn feed(&mut self, data: &[u8], mem: &mut Snapshot) {
        for &c in data {
            match self.state {
                // waiting for the first sync
                0 => {}
                1 => {
                    self.addr = c as u16;
                    self.state = 2;
                }
                2 => {
                    self.addr |= (c as u16) << 8;
                    self.state = if self.addr == 0x5555 { 1 } else { 3 };
                }
                3 => {
                    self.count = c as u16;
                    self.state = 4;
                }
                4 => {
                    self.count |= (c as u16) << 8;
                    self.state = if self.count == 0 { 1 } else { 5 };
                }
                5 => {
                    self.lo = c;
                    self.count = self.count.saturating_sub(1);
                    self.state = 6;
                }
                _ => {
                    mem.insert(self.addr, [self.lo, c]);
                    self.addr = self.addr.wrapping_add(2);
                    self.count = self.count.saturating_sub(1);
                    self.state = if self.count == 0 { 1 } else { 5 };
                }
            }
            if c == 0x55 {
                self.sync += 1;
            } else {
                self.sync = 0;
            }
            if self.sync == 4 {
                self.state = 1;
                self.sync = 0;
            }
        }
    }
}

// ------------------------------------------------------------------ transports

fn tcp_loop() {
    let b = bios();
    let mut warned = false;
    loop {
        let host = b.host.lock().unwrap().clone();
        let port = *b.port.lock().unwrap();
        let addr: Option<SocketAddr> = format!("{host}:{port}").parse().ok();
        let stream = addr.and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(500)).ok());
        let Some(mut stream) = stream else {
            if !warned {
                log(format!("DCS-BIOS: no TCP server on {host}:{port} yet, retrying in the background"));
                warned = true;
            }
            thread::sleep(Duration::from_secs(2));
            continue;
        };
        warned = false;
        log(format!("DCS-BIOS: TCP connected to {host}:{port}"));
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        *b.tcp_out.lock().unwrap() = stream.try_clone().ok();
        let mut parser = Parser::default();
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    log("DCS-BIOS: Connection closed by peer");
                    break;
                }
                Ok(n) => {
                    let mut mem = b.mem.lock().unwrap();
                    parser.feed(&buf[..n], &mut mem);
                    drop(mem);
                    b.mark("TCP");
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => continue,
                Err(e) => {
                    log(format!("DCS-BIOS: Connection closed ({e})"));
                    break;
                }
            }
        }
        *b.tcp_out.lock().unwrap() = None;
        thread::sleep(Duration::from_secs(1));
    }
}

fn udp_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(unix)]
    s.set_reuse_port(true)?;
    s.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, MULTICAST_PORT)).into())?;
    s.join_multicast_v4(&MULTICAST_GROUP, &Ipv4Addr::UNSPECIFIED)?;
    s.set_read_timeout(Some(Duration::from_millis(500)))?;
    Ok(s.into())
}

fn udp_loop() {
    let b = bios();
    let sock = loop {
        match udp_socket() {
            Ok(s) => break s,
            Err(e) => {
                log(format!("DCS-BIOS: UDP multicast listener failed ({e}), retrying in 10 s"));
                thread::sleep(Duration::from_secs(10));
            }
        }
    };
    let mut parser = Parser::default();
    let mut buf = [0u8; 65536];
    loop {
        match sock.recv(&mut buf) {
            Ok(n) if n > 0 => {
                // While TCP delivers, ignore multicast so the same frame is not applied twice.
                if matches!(b.connected(), Some("TCP")) {
                    continue;
                }
                let mut mem = b.mem.lock().unwrap();
                parser.feed(&buf[..n], &mut mem);
                drop(mem);
                b.mark("UDP");
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ commands

/// Send one DCS-BIOS command line, e.g. "UFC_1 1".
pub fn send_message(message: &str) {
    let b = bios();
    let line = format!("{message}\n");
    if let Some(s) = b.tcp_out.lock().unwrap().as_mut() {
        if s.write_all(line.as_bytes()).is_ok() {
            return;
        }
    }
    let host = b.host.lock().unwrap().clone();
    let port = *b.port.lock().unwrap();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.send_to(line.as_bytes(), format!("{host}:{port}"));
    }
}

/// Legacy "TOGGLE" press: the same message twice, 100 ms apart.
pub fn send_button_press(button: &str) {
    send_message(button);
    thread::sleep(BUTTON_SLEEP_DELAY);
    send_message(button);
}

pub fn send_button_state_press(state_1: &str, state_2: &str) {
    send_message(state_1);
    thread::sleep(BUTTON_SLEEP_DELAY);
    send_message(state_2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_handles_split_frames() {
        // sync, block at 0x0000 len 4 "AV8B", block at 0x1000 len 2
        let frame: Vec<u8> = vec![
            0x55, 0x55, 0x55, 0x55, 0x00, 0x00, 0x04, 0x00, b'A', b'V', b'8', b'B', 0x00, 0x10, 0x02,
            0x00, 0x34, 0x12,
        ];
        let mut mem = Snapshot::new();
        let mut p = Parser::default();
        for chunk in frame.chunks(3) {
            p.feed(chunk, &mut mem);
        }
        assert_eq!(mem[&0x0000], [b'A', b'V']);
        assert_eq!(mem[&0x0002], [b'8', b'B']);
        assert_eq!(mem[&0x1000], [0x34, 0x12]);
        assert_eq!(get_string_by_addr_and_len(&mem, 0, 4), "AV8B");
    }
}
