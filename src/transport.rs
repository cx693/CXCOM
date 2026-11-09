//! Background I/O worker: serial, TCP client/server, UDP.

use std::io::{ErrorKind as IoErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::config::Config;

pub enum Cmd {
    Write(Vec<u8>),
    Dtr(bool),
    Rts(bool),
    Stop,
}

pub enum Evt {
    Opened(String),
    Reconnecting(String),
    Data(Vec<u8>),
    Info(String),
    Error(String),
}

pub fn spawn(cfg: &Config) -> (Sender<Cmd>, Receiver<Evt>) {
    let (tx, rx) = mpsc::channel();
    let (evt_tx, evt_rx) = mpsc::channel();
    let cfg = cfg.clone();
    let _ = thread::Builder::new()
        .name("cxcom-io".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let res = match cfg.net_mode {
                0 => serial(&cfg, &rx, &evt_tx),
                1 => tcp_client(&cfg, &rx, &evt_tx),
                2 => tcp_server(&cfg, &rx, &evt_tx),
                _ => udp(&cfg, &rx, &evt_tx),
            };
            if let Err(e) = res {
                let _ = evt_tx.send(Evt::Error(e));
            }
        });
    (tx, evt_rx)
}

fn parity(v: u8) -> Parity {
    match v {
        1 => Parity::Odd,
        2 => Parity::Even,
        _ => Parity::None,
    }
}

fn open_serial(cfg: &Config) -> Result<Box<dyn serialport::SerialPort>, String> {
    serialport::new(&cfg.port, cfg.baud)
        .data_bits(match cfg.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        })
        .stop_bits(if cfg.stop_bits == 2 { StopBits::Two } else { StopBits::One })
        .parity(parity(cfg.parity))
        .flow_control(match cfg.flow {
            1 => FlowControl::Software,
            2 => FlowControl::Hardware,
            _ => FlowControl::None,
        })
        .timeout(Duration::from_millis(20))
        .open()
        .map_err(|e| format!("打开串口失败: {e}"))
}

fn serial(cfg: &Config, cmds: &Receiver<Cmd>, evt: &Sender<Evt>) -> Result<(), String> {
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match open_serial(cfg) {
            Ok(mut port) => {
                let _ = port.write_data_terminal_ready(cfg.dtr);
                let _ = port.write_request_to_send(cfg.rts);
                let _ = evt.send(Evt::Opened(format!("{} 已打开", cfg.port)));

                if !pending.is_empty() {
                    let data = std::mem::take(&mut pending);
                    if let Err(e) = port.write_all(&data) {
                        let msg = format!("串口写入失败: {e}");
                        if !cfg.auto_reconnect {
                            return Err(msg);
                        }
                        let _ = evt.send(Evt::Reconnecting(format!(
                            "{msg}，关闭串口，自动修复中..."
                        )));
                        if !wait_retry(cmds, &mut pending) {
                            return Ok(());
                        }
                        continue;
                    }
                }

                match serial_loop(&mut *port, cmds, evt) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        if !cfg.auto_reconnect {
                            return Err(e);
                        }
                        let _ = evt.send(Evt::Reconnecting(format!(
                            "{e}，关闭串口，自动修复中..."
                        )));
                    }
                }
            }
            Err(e) => {
                if !cfg.auto_reconnect {
                    return Err(e);
                }
                let _ = evt.send(Evt::Reconnecting(format!(
                    "{e}，自动修复中..."
                )));
            }
        }
        if !wait_retry(cmds, &mut pending) {
            return Ok(());
        }
    }
}

/// Wait before the next reconnect attempt, keeping writes queued and
/// honouring a Stop request. Returns false when the worker should exit.
fn wait_retry(cmds: &Receiver<Cmd>, pending: &mut Vec<u8>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        match cmds.recv_timeout(Duration::from_millis(100)) {
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
            Ok(Cmd::Write(d)) => pending.extend_from_slice(&d),
            Ok(Cmd::Dtr(_)) | Ok(Cmd::Rts(_)) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
    true
}

fn serial_loop(
    port: &mut dyn serialport::SerialPort,
    cmds: &Receiver<Cmd>,
    evt: &Sender<Evt>,
) -> Result<(), String> {
    let mut buf = [0u8; 4096];
    loop {
        match cmds.try_recv() {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return Ok(()),
            Ok(Cmd::Write(d)) => {
                if let Err(e) = port.write_all(&d) {
                    return Err(format!("串口写入失败: {e}"));
                }
            }
            Ok(Cmd::Dtr(v)) => {
                let _ = port.write_data_terminal_ready(v);
            }
            Ok(Cmd::Rts(v)) => {
                let _ = port.write_request_to_send(v);
            }
            Err(TryRecvError::Empty) => {}
        }
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                if evt.send(Evt::Data(buf[..n].to_vec())).is_err() {
                    return Ok(());
                }
            }
            Err(e) => {
                let timeout = matches!(
                    e.kind(),
                    IoErrorKind::TimedOut | IoErrorKind::WouldBlock | IoErrorKind::Interrupted
                );
                if !timeout {
                    return Err(format!("串口读取失败: {e}"));
                }
            }
        }
    }
}

fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    (host.trim(), port)
        .to_socket_addrs()
        .map(|it| it.collect())
        .map_err(|e| format!("地址解析失败: {e}"))
}

fn connect_tcp(cfg: &Config) -> Result<TcpStream, String> {
    let addrs = resolve(&cfg.remote_ip, cfg.remote_port)?;
    let mut last = String::from("没有可用地址");
    for a in addrs {
        match TcpStream::connect_timeout(&a, Duration::from_secs(4)) {
            Ok(s) => return Ok(s),
            Err(e) => last = format!("连接失败: {e}"),
        }
    }
    Err(last)
}

fn tcp_client(cfg: &Config, cmds: &Receiver<Cmd>, evt: &Sender<Evt>) -> Result<(), String> {
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match connect_tcp(cfg) {
            Ok(mut s) => {
                let _ = s.set_read_timeout(Some(Duration::from_millis(20)));
                let _ = s.set_nodelay(true);
                let _ = evt.send(Evt::Opened(format!(
                    "已连接 {}:{}",
                    cfg.remote_ip, cfg.remote_port
                )));
                if !pending.is_empty() {
                    let data = std::mem::take(&mut pending);
                    if let Err(e) = s.write_all(&data) {
                        let msg = format!("网络写入失败: {e}");
                        if !cfg.auto_reconnect {
                            return Err(msg);
                        }
                        let _ = evt.send(Evt::Reconnecting(format!("{msg}，自动重连中...")));
                        if !wait_retry(cmds, &mut pending) {
                            return Ok(());
                        }
                        continue;
                    }
                }
                match pump(&mut s, cmds, evt) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        if !cfg.auto_reconnect {
                            return Err(e);
                        }
                        let _ = evt.send(Evt::Reconnecting(format!("{e}，自动重连中...")));
                    }
                }
            }
            Err(e) => {
                if !cfg.auto_reconnect {
                    return Err(e);
                }
                let _ = evt.send(Evt::Reconnecting(format!("{e}，自动重连中...")));
            }
        }
        if !wait_retry(cmds, &mut pending) {
            return Ok(());
        }
    }
}

fn pump(s: &mut TcpStream, cmds: &Receiver<Cmd>, evt: &Sender<Evt>) -> Result<(), String> {
    let mut buf = [0u8; 8192];
    loop {
        match cmds.try_recv() {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return Ok(()),
            Ok(Cmd::Write(d)) => {
                s.write_all(&d).map_err(|e| format!("网络写入失败: {e}"))?;
            }
            Ok(Cmd::Dtr(_)) | Ok(Cmd::Rts(_)) => {}
            Err(TryRecvError::Empty) => {}
        }
        match s.read(&mut buf) {
            Ok(0) => return Err("对端已关闭连接".into()),
            Ok(n) => {
                if evt.send(Evt::Data(buf[..n].to_vec())).is_err() {
                    return Ok(());
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    IoErrorKind::WouldBlock | IoErrorKind::TimedOut | IoErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(format!("网络读取失败: {e}")),
        }
    }
}

fn tcp_server(cfg: &Config, cmds: &Receiver<Cmd>, evt: &Sender<Evt>) -> Result<(), String> {
    let listener = TcpListener::bind(("0.0.0.0", cfg.local_port))
        .map_err(|e| format!("绑定端口失败: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let _ = evt.send(Evt::Opened(format!("监听中，本地端口 {}", cfg.local_port)));
    let mut client: Option<TcpStream> = None;
    let mut buf = [0u8; 8192];
    loop {
        match cmds.try_recv() {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return Ok(()),
            Ok(Cmd::Write(d)) => match client.as_mut() {
                Some(s) => {
                    if s.write_all(&d).is_err() {
                        client = None;
                    }
                }
                None => {
                    let _ = evt.send(Evt::Info("还没有客户端连接".into()));
                }
            },
            Ok(Cmd::Dtr(_)) | Ok(Cmd::Rts(_)) => {}
            Err(TryRecvError::Empty) => {}
        }
        match listener.accept() {
            Ok((s, _)) => {
                let _ = s.set_read_timeout(Some(Duration::from_millis(20)));
                let _ = s.set_nodelay(true);
                let _ = evt.send(Evt::Info("客户端已连接".into()));
                client = Some(s);
            }
            Err(e) if e.kind() == IoErrorKind::WouldBlock => {}
            Err(e) => {
                let _ = evt.send(Evt::Info(format!("accept 失败: {e}")));
            }
        }
        if let Some(s) = client.as_mut() {
            match s.read(&mut buf) {
                Ok(0) => {
                    client = None;
                    let _ = evt.send(Evt::Info("客户端已断开".into()));
                }
                Ok(n) => {
                    if evt.send(Evt::Data(buf[..n].to_vec())).is_err() {
                        return Ok(());
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        IoErrorKind::WouldBlock | IoErrorKind::TimedOut | IoErrorKind::Interrupted
                    ) => {}
                Err(_) => {
                    client = None;
                }
            }
        } else {
            thread::sleep(Duration::from_millis(4));
        }
    }
}

fn udp(cfg: &Config, cmds: &Receiver<Cmd>, evt: &Sender<Evt>) -> Result<(), String> {
    let sock = UdpSocket::bind(("0.0.0.0", cfg.local_port)).map_err(|e| format!("UDP 绑定失败: {e}"))?;
    let _ = sock.set_read_timeout(Some(Duration::from_millis(20)));
    let remote = resolve(&cfg.remote_ip, cfg.remote_port)?
        .into_iter()
        .next()
        .ok_or("远程地址无效")?;
    let _ = evt.send(Evt::Opened(format!("UDP 本地 {} <-> {remote}", cfg.local_port)));
    let mut buf = [0u8; 8192];
    loop {
        match cmds.try_recv() {
            Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return Ok(()),
            Ok(Cmd::Write(d)) => {
                sock.send_to(&d, remote).map_err(|e| format!("UDP 发送失败: {e}"))?;
            }
            Ok(Cmd::Dtr(_)) | Ok(Cmd::Rts(_)) => {}
            Err(TryRecvError::Empty) => {}
        }
        match sock.recv_from(&mut buf) {
            Ok((n, src)) if src.ip() == remote.ip() && src.port() == remote.port() => {
                if evt.send(Evt::Data(buf[..n].to_vec())).is_err() {
                    return Ok(());
                }
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    IoErrorKind::WouldBlock | IoErrorKind::TimedOut | IoErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(format!("UDP 接收失败: {e}")),
        }
    }
}
