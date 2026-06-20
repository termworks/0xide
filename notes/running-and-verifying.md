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
