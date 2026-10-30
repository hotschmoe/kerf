//! `kerf serve` startup: arguments, token, bind, the poller, the accept loop.

use super::conn::{MAX_CONNS, conn_watchdog, serve_conn};
use super::{Config, POLL_MS, SERVE_USAGE, Server, agent, ui};
use crate::agents::{self, Manager};
use crate::events::Hub;
use crate::sys;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static SHUTDOWN_SIGNAL: AtomicU8 = AtomicU8::new(0);

fn poller(s: Arc<Server>) {
    let mut tick = 0u32;
    loop {
        thread::sleep(Duration::from_millis(POLL_MS));
        let sig = SHUTDOWN_SIGNAL.load(Ordering::Acquire);
        if sig != 0 {
            s.agent_mgr.shutdown();
            std::process::exit(128 + i32::from(sig));
        }
        s.scan(true);
        tick += 1;
        if tick.is_multiple_of(30) {
            s.hub.publish("ping", "{}");
        }
    }
}

fn refuse(mut stream: TcpStream) {
    let _ = stream.write_all(
        b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nRetry-After: 2\r\nContent-Length: 0\r\n\r\n",
    );
}

fn accept_loop(s: &Arc<Server>, listener: &TcpListener) {
    for stream in listener.incoming() {
        let stream = if let Ok(st) = stream {
            st
        } else {
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        if s.active_conns.fetch_add(1, Ordering::AcqRel) >= MAX_CONNS {
            s.active_conns.fetch_sub(1, Ordering::AcqRel);
            refuse(stream);
            continue;
        }
        let s2 = s.clone();
        let spawned = thread::Builder::new().stack_size(8 << 20).spawn(move || serve_conn(s2, stream));
        if spawned.is_err() {
            s.active_conns.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

fn open_browser(url: &str) {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    if let Ok(mut child) = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        let _ = child.wait();
    }
}

fn lan_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

fn port_in_use(host: &str, port: u16) -> bool {
    let probe = match host {
        "0.0.0.0" => "127.0.0.1",
        "::" => "::1",
        h => h,
    };
    let Ok(ip) = probe.parse::<IpAddr>() else { return false };
    TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(500)).is_ok()
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "::1" || host.starts_with("127.")
}

fn random_token() -> Option<String> {
    let mut raw = [0u8; 16];
    sys::random_bytes(&mut raw).then_some(())?;
    Some(raw.iter().map(|b| format!("{b:02x}")).collect())
}

fn confirm_workspace_agents(dir: &Path, dir_abs: &Path) -> bool {
    let templates = agents::load_templates(dir, false);
    let mut n = 0;
    for t in templates.iter().filter(|t| t.from_workspace) {
        if n == 0 {
            eprintln!(
                "kerf serve: {}/.kerf/agents.json defines custom agents; starting or detecting them runs commands from this folder:",
                dir_abs.display()
            );
        }
        n += 1;
        eprintln!(
            "  - {}: detect `{}`, run `{}`",
            t.id,
            t.detect.first().map_or("", String::as_str),
            t.argv.first().map_or("", String::as_str)
        );
    }
    if n == 0 {
        return false;
    }
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        eprintln!(
            "  not trusted (no terminal to ask): they are listed as untrusted and never started. Pass --trust-agents if you wrote this file."
        );
        return false;
    }
    eprint!("Trust these agents? [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    let ans = line.trim();
    let yes = ans.eq_ignore_ascii_case("y") || ans.eq_ignore_ascii_case("yes");
    if !yes {
        eprintln!("  not trusted: they stay listed as untrusted and never start.");
    }
    yes
}

fn usage_err(msg: &str) -> u8 {
    eprint!("kerf serve: {msg}\n{SERVE_USAGE}");
    2
}

pub fn cli_main(args: &[String]) -> u8 {
    let mut cfg = Config::default();
    let (mut no_token, mut token_file) = (false, None::<String>);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        let takes_value = [
            "--dir",
            "--host",
            "--port",
            "--token",
            "--allow-origin",
            "--agent-timeout",
            "--token-file",
            "--timeouts-ms",
        ]
        .contains(&a);
        if takes_value {
            let Some(v) = args.get(i) else { return usage_err(&format!("{a} needs a value")) };
            i += 1;
            match a {
                "--timeouts-ms" => {
                    let mut it = v.split(',').map(|x| x.parse::<u32>().unwrap_or(0));
                    let (t_idle, t_head, t_write) =
                        (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0));
                    if t_idle == 0 || t_head == 0 || t_write == 0 {
                        eprintln!("kerf serve: --timeouts-ms wants IDLE,HEAD,WRITE in milliseconds, got '{v}'");
                        return 2;
                    }
                    cfg.timeouts = super::conn::Timeouts { idle_ms: t_idle, head_ms: t_head, write_ms: t_write };
                }
                "--token-file" => token_file = Some(v.clone()),
                "--agent-timeout" => {
                    let Ok(n) = v.parse::<u32>() else {
                        eprintln!("kerf serve: --agent-timeout must be a number of seconds, got '{v}'");
                        return 2;
                    };
                    cfg.agent_timeout_s = n;
                }
                "--dir" => cfg.dir_path = v.clone(),
                "--host" => cfg.host = v.clone(),
                "--token" => cfg.token = Some(v.clone()),
                "--allow-origin" => cfg.allow_origin = Some(v.clone()),
                _ => {
                    let Ok(p) = v.parse::<u16>() else {
                        eprintln!("kerf serve: --port must be 0..65535, got '{v}'");
                        return 2;
                    };
                    cfg.port = p;
                }
            }
        } else {
            match a {
                "--open" => cfg.open = true,
                "--no-token" => no_token = true,
                "--trust-agents" => cfg.trust_agents = true,
                "-h" | "--help" => {
                    eprint!("{SERVE_USAGE}");
                    return 0;
                }
                _ => return usage_err(&format!("unknown option {a}")),
            }
        }
    }
    if let Some(tf) = &token_file {
        let mut raw = Vec::new();
        match std::fs::File::open(tf) {
            Ok(f) => {
                if f.take(4096).read_to_end(&mut raw).is_err() {
                    eprintln!("kerf serve: cannot read --token-file '{tf}'");
                    return 1;
                }
            }
            Err(e) => {
                eprintln!("kerf serve: cannot read --token-file '{tf}': {}", io_error_name(&e));
                return 1;
            }
        }
        cfg.token = Some(String::from_utf8_lossy(&raw).trim_matches([' ', '\t', '\r', '\n']).to_string());
    } else if cfg.token.is_none() {
        if let Some(t) = std::env::var("KERF_TOKEN").ok().filter(|t| !t.is_empty()) {
            cfg.token = Some(t);
        }
    } else {
        eprintln!("kerf serve: note: --token is visible to other users in `ps`; prefer KERF_TOKEN or --token-file");
    }
    if no_token && cfg.token.is_some() {
        eprintln!("kerf serve: use either a token (--token, --token-file, KERF_TOKEN) or --no-token");
        return 2;
    }
    if !no_token && cfg.token.is_none() {
        let Some(t) = random_token() else {
            eprintln!(
                "kerf serve: no secure random source for the access token; pass --token-file, or --no-token on a trusted machine"
            );
            return 1;
        };
        cfg.token = Some(t);
    }
    if cfg.token.as_deref() == Some("") {
        eprintln!("kerf serve: the token must not be empty");
        return 2;
    }
    let loopback = is_loopback_host(&cfg.host);

    let dir_abs = match std::fs::metadata(&cfg.dir_path)
        .and_then(|m| if m.is_dir() { Ok(()) } else { Err(io::ErrorKind::NotADirectory.into()) })
        .and_then(|()| std::fs::canonicalize(&cfg.dir_path))
        .map(sys::plain_path)
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("kerf serve: cannot open --dir '{}': {}", cfg.dir_path, io_error_name(&e));
            return 1;
        }
    };
    if !cfg.trust_agents {
        cfg.trust_agents = confirm_workspace_agents(&dir_abs, &dir_abs);
    }
    let bind_host = if cfg.host.eq_ignore_ascii_case("localhost") { "127.0.0.1".to_string() } else { cfg.host.clone() };
    let Ok(ip) = bind_host.parse::<IpAddr>() else {
        eprintln!("kerf serve: --host must be an IP address (127.0.0.1, 0.0.0.0, ::1 …), got '{}'", cfg.host);
        return 2;
    };
    let in_use = |port: u16| format!("kerf serve: port {port} is already in use; try --port {}", port.wrapping_add(1));
    if cfg.port != 0 && port_in_use(&bind_host, cfg.port) {
        eprintln!("{}", in_use(cfg.port));
        return 1;
    }
    let listener = match TcpListener::bind(SocketAddr::new(ip, cfg.port)) {
        Ok(l) => l,
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            eprintln!("{}", in_use(cfg.port));
            return 1;
        }
        Err(e) => {
            eprintln!("kerf serve: cannot listen on {}:{}: {}", cfg.host, cfg.port, io_error_name(&e));
            return 1;
        }
    };
    let port = listener.local_addr().map_or(cfg.port, |a| a.port());
    cfg.port = port;
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();

    let hub = Arc::new(Hub::default());
    let server = Arc::new_cyclic(|weak: &std::sync::Weak<Server>| {
        let mut mgr = Manager::new(hub.clone(), dir_abs.clone(), exe_dir, cfg.trust_agents, cfg.agent_timeout_s);
        let w = weak.clone();
        mgr.set_before_exit(move || {
            if let Some(s) = w.upgrade() {
                s.scan(true);
            }
        });
        Server {
            dir_abs: dir_abs.clone(),
            hub: hub.clone(),
            scan: Mutex::default(),
            write_mu: Mutex::default(),
            agent_mgr: Arc::new(mgr),
            loopback_only: loopback,
            started: Instant::now(),
            conns: Mutex::default(),
            active_conns: Default::default(),
            sse_active: Default::default(),
            ui_csp: if ui::has_ui() { ui::build_ui_csp() } else { ui::default_ui_csp() },
            cfg,
        }
    });
    server.scan(false);
    agent::cleanup_attachments(&server);
    sys::install_shutdown_handler(&SHUTDOWN_SIGNAL);

    let local_host = if loopback { bind_host.as_str() } else { "127.0.0.1" };
    let dir_text = dir_abs.display();
    let mut out = String::new();
    match &server.cfg.token {
        Some(t) => out.push_str(&format!("kerf serve: http://{local_host}:{port}/?token={t}  (dir {dir_text})\n")),
        None => out.push_str(&format!("kerf serve: http://{local_host}:{port}/  (dir {dir_text})\n")),
    }
    if !loopback {
        let any = bind_host == "0.0.0.0" || bind_host == "::";
        let shown = if any {
            lan_ip().map_or_else(|| "<this-machine-ip>".to_string(), |ip| ip.to_string())
        } else {
            bind_host.clone()
        };
        match &server.cfg.token {
            Some(t) => out.push_str(&format!("LAN:  http://{shown}:{port}/?token={t}\n")),
            None => out.push_str(&format!(
                "LAN:  http://{shown}:{port}/   (NO TOKEN: anyone on this network can edit the folder and run agents)\n"
            )),
        }
    }
    if !ui::has_ui() {
        out.push_str(
            "note: this binary has no embedded web UI (build with KERF_UI=<dist dir>); the API is available.\n",
        );
    }
    let mut stdout = io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();

    {
        let s = server.clone();
        thread::spawn(move || poller(s));
        let s = server.clone();
        thread::spawn(move || conn_watchdog(s));
        let s = server.clone();
        thread::spawn(move || {
            for t in agents::load_templates(&s.dir_abs, s.cfg.trust_agents) {
                let _ = s.agent_mgr.detect_cached(&t);
            }
        });
    }
    if server.cfg.open {
        let url = match &server.cfg.token {
            Some(t) => format!("http://{local_host}:{port}/?token={t}"),
            None => format!("http://{local_host}:{port}/"),
        };
        open_browser(&url);
    }
    accept_loop(&server, &listener);
    0
}

fn io_error_name(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "FileNotFound",
        io::ErrorKind::PermissionDenied => "AccessDenied",
        io::ErrorKind::NotADirectory => "NotDir",
        io::ErrorKind::AddrInUse => "AddressInUse",
        _ => "Unexpected",
    }
}
