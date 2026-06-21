# Running & verifying snertwl (verified 2026-06-20)

## Nested (fast dev loop)
- We run inside a Hyprland Wayland session, so `wlr_backend_autocreate` picks the
  **nested Wayland backend** with no extra config: it opens one output `WL-1` as a
  window (default 1280x720) on the host desktop.
- Command: `cargo nested` (alias for `cargo run`). Ctrl-C to quit.
- Render path confirmed: GLES2 renderer on Intel Iris Xe → GBM buffer (format XR24)
  → GL FBO → DMA-BUF imported into the parent compositor.

## Verifying the server with a client (Stage 1+)
- `main` opens a socket via `wl_display_add_socket_auto` and, if given argv,
  spawns that program with `WAYLAND_DISPLAY` set to our socket.
- One-command check: `cargo nested -- wayland-info` — the client connects and
  lists our globals (wl_shm, zwp_linux_dmabuf_v1, wl_compositor, wl_subcompositor,
  wl_data_device_manager). Its stdout interleaves with the wlroots debug log.
- Note: `wlr_compositor_create` makes only wl_compositor; wl_shm + linux-dmabuf
  come from `wlr_renderer_init_wl_display(renderer, display)`.
- Gotcha: real apps (e.g. `foot`) refuse to start without a `wl_seat` global
  ("no seats available") — we create a minimal one via `snertwl_seat_create`.
- Gotcha: wlroots' xdg-shell header #includes `xdg-shell-protocol.h`, which is
  NOT a system header — build.rs generates it with `wayland-scanner` into OUT_DIR.
- Input verification: no `wtype`/`ydotool` installed, so keystrokes can't be
  injected from the agent. Verify keyboard *wiring* via log markers
  ("keyboard attached", "keyboard focus -> toplevel"); verify actual typing by
  hand — focus the nested snertwl window in the host, then type into foot.
- Nested keyboard caveat: the Wayland backend only receives keys when the host
  (Hyprland) gives the snertwl window focus.

## Config file (Stage 5d)
- Parsed in Rust (`src/config.rs`), no extra crates. Path:
  `$XDG_CONFIG_HOME/snertwl/snertwl.conf` (else `~/.config/snertwl/snertwl.conf`).
  No file → built-in defaults (Super, gap 10, the original keymap); a config with
  zero `bind=` lines also falls back to the default binds.
- Format `key = value`, `#` comments. Scalars: `modifier`, `gap`, `background`.
  Binds: `bind = MODS, KEY, ACTION[, ARG]` (Hyprland-ish). `MOD`/`$mod` in a bind
  expands to the primary `modifier`. KEY names resolve via xkb
  (`snertwl_keysym_from_name` shim → `xkb_keysym_from_name`, case-insensitive →
  level-0 keysym, matching how `handle_key` reports presses).
- `SNERTWL_MOD=alt` still overrides the modifier (applied *before* binds are
  parsed, so `MOD` resolves to Alt) — keep using it for nested dev.
- An unparseable line warns on stderr (`config line N: …`) and is skipped; it
  never stops startup. See `snertwl.conf.example` in the repo root.
- Verify without a window: `XDG_CONFIG_HOME=/tmp/cfg WLR_BACKENDS=headless \
  target/debug/snertwl >log 2>&1 &` then grep `snertwl: (loaded|no config|modifier|config line)`.

## Multi-output (Stage 6a)
- Each output (monitor) is tracked in Rust (`Server.outputs: Vec<Output>` with its
  layout box x/y/w/h + the workspace it shows). Tiling is per-output: `refresh()`
  hides windows whose workspace isn't displayed anywhere, then tiles each output's
  workspace within that output's box. New outputs grab the lowest free workspace.
- `switch_workspace` acts on `focused_output`; if the target is already on another
  monitor the two outputs *swap* workspaces (so no workspace shows on two monitors).
- Each output's background rect is positioned at its layout origin (the shim's
  `snertwl_scene_add_output_background` now takes x,y) — without this a second
  output's background sits at 0,0 and its window renders black.
- Verify nested with two outputs: the Wayland backend honors `WLR_WL_OUTPUTS=2`,
  opening two host windows.
  `WLR_WL_OUTPUTS=2 SNERTWL_MOD=alt target/debug/snertwl foot` →
  output 0 shows the foot tiled (ws1), output 1 shows the bare background (ws2).
  Log: two `output online @ X,Y WxH — workspace N` lines.
- Known gaps (full Stage 6, deferred): `focused_output` is always 0 (no
  focus-follows-monitor yet, so keyboard spawns land on the first monitor);
  no output *removal*/hotplug-unplug handling (don't unplug a monitor mid-session);
  no session/VT-switch handling (that's Stage 6b).

## Real hardware (TTY / DRM-KMS) — works as of 2026-06-20
- `wlr_backend_autocreate` picks the DRM/KMS backend on a bare TTY (no WAYLAND_DISPLAY).
- Recipe: log into a free VT, then
  `LIBSEAT_BACKEND=logind ~/Projects/snertwl/target/debug/snertwl foot 2>~/snertwl-tty.log`
  - `LIBSEAT_BACKEND=logind` because user isn't in the `seat` group (logind grants the
    active VT its devices). Two GPUs here: Intel `card1` (panel), discrete `card0`;
    prepend `WLR_DRM_DEVICES=/dev/dri/card1` if it picks the wrong one.
- **Single display works** (tile/focus/close/quit). **Multi-monitor is broken** —
  `relayout()` only knows one output's size; proper multi-output + VT-switch handling
  is deferred to full Stage 6.

## Headless verification recipe (for automated/agent checks)
Because a nested run opens a window on the host and then blocks in `wl_display_run`,
verify it like this:
```sh
target/debug/snertwl >/tmp/snertwl.log 2>&1 &   # launch, capture wlroots debug log
PID=$!; sleep 3
grim /tmp/snertwl.png                            # screenshot the host screen (incl. our window)
kill $PID
```
Then read the PNG (our window is a solid-color rect) and grep the log for our
`println!` markers + `Allocated ... GBM buffer` / `DMA-BUF imported` lines.

- wlroots debug logging is on via `snertwl_log_init()` → very verbose (the first
  ~450 lines are EGL/DMA-BUF format enumeration; the interesting events are at the end).
- A true headless backend run (`WLR_BACKENDS=headless`) creates **zero** outputs by
  default — needs `WLR_HEADLESS_OUTPUTS=1`, and has no window to screenshot. Deferred
  until we have screencopy or a socket for `grim` to attach to.
