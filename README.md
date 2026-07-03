# 0xide

**A from-scratch tiling Wayland compositor, written in Rust on top of [wlroots](https://gitlab.freedesktop.org/wlroots/wlroots).**

0xide is a personal, learning-first compositor built directly on wlroots 0.19 rather
than on top of any desktop. It's a **dynamic tiling** compositor — windows are arranged
automatically to fill the screen instead of floating and overlapping.

> **Status:** early but real. It runs nested inside another Wayland session for
> development, and on actual hardware as a real DRM/KMS session on a TTY. It is being
> grown into something to daily-drive, one capability at a time.

## What works now

- **Spiral / dwindle tiling** — each new window splits the remaining space, alternating
  vertical (left/right) then horizontal (top/bottom).
- **9 workspaces** — switch between them and move windows across them from the keyboard.
- **Multi-monitor** with **focus-follows-monitor** — new windows open on the monitor
  your cursor is on; each monitor shows its own workspace.
- **Keyboard-driven**, configured by a small **Rust-parsed config file** (modifier,
  gaps, background colour, keybindings, terminal command).
- **Pointer + cursor** with click-to-focus.
- Runs real **xdg-shell apps** (terminals, browsers, …).
- Runs on a **real TTY** via libseat/logind, and **survives VT switching**
  (Ctrl+Alt+Fn away and back) without crashing or losing your windows.
- **Layer-shell** (`wlr-layer-shell-unstable-v1`) — bars, panels and wallpaper (e.g.
  [quickshell](https://quickshell.org)) render in the correct z-order and reserve their
  screen space, so tiled windows never sit underneath them.

## Architecture

The split is deliberate:

- **Rust owns all policy** — the window list, tiling layout, workspaces, keybindings,
  config parsing, and overall flow (`src/main.rs`, `src/config.rs`).
- **A thin C shim** (`shim/oxide_shim.{c,h}`) owns the parts that are awkward or
  unsafe to model through FFI: the wlroots `wl_listener`/`wl_signal` glue (intrusive
  linked lists) and anything that needs to read wlroots struct fields directly. It
  exposes clean `(userdata, data)` callbacks to Rust.
- **wlroots** is the C library doing the heavy lifting (DRM/KMS modesetting, the GLES2
  renderer, libinput, the scene graph, protocol plumbing). We bind to it with
  `bindgen` + the shim; we don't rewrite it.

In short: **wlroots = mechanism, 0xide = policy.** See
[`notes/architecture.md`](notes/architecture.md) for the full division of labour.

## Build

Built and run on **Arch Linux**. System dependencies:

```
wlroots0.19 wayland wayland-protocols libxkbcommon libinput libdrm seatd mesa pixman pkgconf clang
```

The Rust toolchain is pinned in `rust-toolchain.toml`. Then:

```sh
cargo build
```

The build script (`build.rs`) finds wlroots via `pkg-config`, generates the
`xdg-shell` protocol header with `wayland-scanner`, compiles the C shim, and runs
`bindgen` over `wrapper.h`.

## Run

### Nested (the fast dev loop)

Inside an existing Wayland session, 0xide opens as a window:

```sh
OXIDE_MOD=alt cargo nested -- kitty
```

`cargo nested` is an alias for `cargo run`. `OXIDE_MOD=alt` makes the modifier key
**Alt** instead of Super, because the host compositor grabs Super-chords before 0xide
sees them. The trailing `-- kitty` launches a test client against 0xide's socket.

### On a real display (TTY / DRM-KMS)

From a free virtual terminal (e.g. Ctrl+Alt+F5), logged in:

```sh
LIBSEAT_BACKEND=logind ~/Projects/0xide/target/debug/0xide kitty 2>~/0xide-tty.log
```

`LIBSEAT_BACKEND=logind` lets logind grant the active VT its devices (no `seat` group
needed). Here the modifier is the real **Super** key. Ctrl+Alt+F1 gets you back to your
main session. More detail and verification recipes are in
[`notes/running-and-verifying.md`](notes/running-and-verifying.md).

## Default keybindings

`Mod` is **Super** by default (**Alt** when running nested with `OXIDE_MOD=alt`).

| Keys                | Action                              |
| ------------------- | ----------------------------------- |
| `Mod + Return`      | Open the terminal                   |
| `Mod + Q`           | Close the focused window            |
| `Mod + Shift + Q`   | Quit 0xide                        |
| `Mod + J` / `Mod + K` | Focus next / previous window      |
| `Mod + 1…9`         | Switch to workspace 1–9             |
| `Mod + Shift + 1…9` | Move focused window to workspace 1–9 |
| `Ctrl + Alt + F1…F12` | Switch virtual terminal           |

## Configuration

0xide reads `~/.config/0xide/0xide.conf` (or `$XDG_CONFIG_HOME/0xide/0xide.conf`).
With no config file it uses the built-in defaults above. The format is `key = value`
with `#` comments, plus `bind` lines:

```
modifier   = super
gap        = 10
background = 0.0 0.6 0.6

bind = MOD, Return, spawn, kitty
bind = MOD, Q, close
bind = MOD SHIFT, Q, quit
bind = MOD, 1, workspace, 1
bind = MOD SHIFT, 1, movetoworkspace, 1
```

A line 0xide can't parse is warned about on stderr and skipped — never fatal. See
[`0xide.conf.example`](0xide.conf.example) for the full annotated example.

## Repository layout

| Path                      | What it is                                                |
| ------------------------- | --------------------------------------------------------- |
| `src/main.rs`             | Compositor orchestrator + all policy (layout, workspaces, input, keybindings) |
| `src/config.rs`           | Dependency-free config-file parser                        |
| `shim/oxide_shim.{c,h}` | Thin C shim: wlroots listener glue + struct access        |
| `build.rs`, `wrapper.h`   | The FFI pipeline (pkg-config, wayland-scanner, cc, bindgen) |
| `notes/`                  | Architecture, toolchain, and run/verify notes             |
| `KICKOFF.md`              | The project's mission and learning-first working rules    |

---

0xide is a personal, learning-first project — built concept-by-concept with every
file and function understood rather than assembled. Its working rules live in
[`KICKOFF.md`](KICKOFF.md). No license yet.
