# deep

A task manager for the terminal that's easier and goes deeper than Windows Task Manager. Kill processes or whole process trees, see where every process runs from, and find what's using a port.

## Install

**Windows** (PowerShell):

```powershell
irm https://github.com/B33BMO/deep/releases/latest/download/install.ps1 | iex
```

**Linux** (x64):

```sh
curl -fsSL https://github.com/B33BMO/deep/releases/latest/download/install.sh | sh
```

Or grab `deep-windows-x64.exe` straight from the [latest release](https://github.com/B33BMO/deep/releases/latest). It's a single file with nothing to install. To build from source instead: `cargo install --git https://github.com/B33BMO/deep`.

Run `deep`. Run it as Administrator to see system process paths and kill system processes.

## Features

- Live process table with CPU (scaled like Task Manager), memory, disk speed, ports, user and full exe path
- Tree view with collapsible branches
- Kill a process or its whole tree, with a guard on critical system processes
- Details pane: exe, working folder, command line, parent chain, children, network connections
- Ports tab: every TCP/UDP socket and the process that owns it
- Open a process's folder in Explorer, copy its path, command line or PID, view its environment variables

## Keys

| Key | Action |
| --- | --- |
| `1` `2` / `Tab` | Processes / Ports |
| `/` | Filter as you type (`Esc` clears) |
| `k` / `Del` | Kill selected process |
| `K` / `Shift+Del` | Kill process and its whole tree |
| `t` | Tree view (`←` `→` collapse/expand) |
| `o` | Open file location |
| `c` `C` `i` | Copy path / command line / PID |
| `e` | Environment variables |
| `p` / `Enter` | Process's ports / socket's process |
| `u` | Jump to parent |
| `l` | Ports: listening only |
| `s` `r` | Sort column / reverse |
| `d` | Toggle details pane |
| `Space` | Pause |
| `?` | All keys |

Filter tricks: in Processes, `port:8080` lists processes using port 8080. In Ports, `443` shows everything on port 443 and `pid:1234` shows one process's sockets.
