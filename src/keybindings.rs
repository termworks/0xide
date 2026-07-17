//! Keybinding dispatch: VT switching, and the config's bind table.

use crate::config::{Action, Direction, MOD_ALT, MOD_CTRL, MOD_MASK};
use crate::ffi::*;
use crate::state::*;
use crate::tiling::{active_output, active_workspace, refresh, spatial_neighbor};
use crate::toplevel::{set_floating, set_fullscreen};
use crate::wlr;
use std::os::raw::c_void;
use std::process::Command;

// Function-key keysyms (contiguous): F1 = 0xffbe … F12 = 0xffc9.
const KEY_F1: u32 = 0xffbe;
const KEY_F12: u32 = 0xffc9;

/// Give keyboard focus to window `idx` (wrapped) of the focused output's
/// workspace.
pub(crate) unsafe fn focus_index(server: &mut Server, idx: usize) {
    if server.outputs.is_empty() {
        return;
    }
    let a = active_workspace(server);
    let len = server.workspaces[a].windows.len();
    if len == 0 {
        return;
    }
    let i = idx % len;
    server.workspaces[a].focused = i;
    oxide_focus_toplevel(server.seat, (*server.workspaces[a].windows[i]).xdg_toplevel);
}

/// Ask the focused window of the focused output's workspace to close.
unsafe fn close_focused(server: &Server) {
    if server.outputs.is_empty() {
        return;
    }
    let ws = &server.workspaces[active_workspace(server)];
    if let Some(&tl) = ws.windows.get(ws.focused) {
        wlr::wlr_xdg_toplevel_send_close((*tl).xdg_toplevel);
    }
}

/// Display `target` on the focused output. If it's already shown on another
/// output, swap the two outputs' workspaces (so no workspace is on two monitors).
unsafe fn switch_workspace(server: &mut Server, target: usize) {
    if server.outputs.is_empty() || target >= server.workspaces.len() {
        return;
    }
    let fo = active_output(server);
    let current = server.outputs[fo].workspace;
    if target == current {
        return;
    }
    if let Some(other) = server.outputs.iter().position(|o| o.workspace == target) {
        server.outputs[other].workspace = current; // swap: that monitor takes ours
    }
    server.outputs[fo].workspace = target;
    refresh(server);
    let f = server.workspaces[target].focused;
    focus_index(server, f);
    eprintln!("0xide: output {} -> workspace {}", fo, target + 1);
}

/// Move the focused output's focused window to another workspace.
unsafe fn move_to_workspace(server: &mut Server, target: usize) {
    if server.outputs.is_empty() || target >= server.workspaces.len() {
        return;
    }
    let a = active_workspace(server);
    if target == a || server.workspaces[a].windows.is_empty() {
        return;
    }
    let focused = server.workspaces[a].focused;
    let tl = server.workspaces[a].windows.remove(focused);
    let len = server.workspaces[a].windows.len();
    if server.workspaces[a].focused >= len && len > 0 {
        server.workspaces[a].focused = len - 1;
    }
    server.workspaces[target].windows.push(tl);
    refresh(server); // recomputes visibility (target may or may not be displayed)
    let f = server.workspaces[a].focused;
    focus_index(server, f);
    eprintln!("0xide: moved window to workspace {}", target + 1);
}

/// How far one Mod+Shift+hjkl press moves a floating window, in pixels.
const NUDGE_STEP: i32 = 50;

/// Move a floating window one step in `dir`, kept within the usable area of
/// the output currently showing its workspace (so it can't be pushed under a
/// bar or off the screen).
unsafe fn nudge_floating(server: &mut Server, tl: *mut Toplevel, dir: Direction) {
    let ws_idx = server
        .workspaces
        .iter()
        .position(|ws| ws.windows.contains(&tl))
        .expect("nudged window must be in a workspace");
    let Some(o) = server.outputs.iter().find(|o| o.workspace == ws_idx) else {
        return; // workspace not on any output right now
    };
    let (mut x, mut y) = ((*tl).x, (*tl).y);
    match dir {
        Direction::Left => x -= NUDGE_STEP,
        Direction::Right => x += NUDGE_STEP,
        Direction::Up => y -= NUDGE_STEP,
        Direction::Down => y += NUDGE_STEP,
    }
    x = x.clamp(o.ux, (o.ux + o.uw - (*tl).w).max(o.ux));
    y = y.clamp(o.uy, (o.uy + o.uh - (*tl).h).max(o.uy));
    oxide_scene_tree_set_position((*tl).scene_tree, x, y);
    ((*tl).x, (*tl).y) = (x, y);
}

/// Launch a program as a client of 0xide (inherits our WAYLAND_DISPLAY). Runs
/// through a shell (like Hyprland's `exec`) so `~`, env vars, `&&`, and quoting
/// in bind commands work as expected — a plain `execvp` doesn't expand any of
/// that.
fn spawn(cmd: &str) {
    if let Err(e) = Command::new("sh").arg("-c").arg(cmd).spawn() {
        eprintln!("0xide: failed to spawn `{cmd}`: {e}");
    }
}

/// Called by the shim for each key press; returns true to consume the key.
/// We look the (modifiers, keysym) up in the config's bind table; an unmatched
/// chord falls through to the focused app.
pub(crate) unsafe extern "C" fn handle_keybinding(
    userdata: *mut c_void,
    keysym: u32,
    modifiers: u32,
) -> bool {
    let server = &mut *(userdata as *mut Server);
    let mods = modifiers & MOD_MASK;

    // VT switching (Ctrl+Alt+F1..F12). Handled before config binds and always
    // consumed; the shim no-ops it when there's no session (nested).
    if mods == MOD_CTRL | MOD_ALT && (KEY_F1..=KEY_F12).contains(&keysym) {
        oxide_session_change_vt(server.session, keysym - KEY_F1 + 1);
        return true;
    }

    // Find the matching bind, then act. We clone the action first so the
    // immutable borrow of `server.config` ends before we mutate `server`.
    let action = server
        .config
        .binds
        .iter()
        .find(|b| b.mods == mods && b.keysym == keysym)
        .map(|b| b.action.clone());
    let Some(action) = action else { return false };

    // Window count on the focused output's workspace (0 if no output yet).
    let n = if server.outputs.is_empty() {
        0
    } else {
        server.workspaces[active_workspace(server)].windows.len()
    };
    match action {
        Action::Spawn(cmd) => spawn(&cmd),
        Action::Close => close_focused(server),
        Action::Quit => wlr::wl_display_terminate(server.display),
        Action::FocusNext if n > 0 => {
            let f = server.workspaces[active_workspace(server)].focused;
            focus_index(server, f + 1);
        }
        Action::FocusPrev if n > 0 => {
            let f = server.workspaces[active_workspace(server)].focused;
            focus_index(server, f + n - 1);
        }
        Action::FocusNext | Action::FocusPrev => {}
        Action::MoveFocus(dir) if n > 0 => {
            let a = active_workspace(server);
            let f = server.workspaces[a].focused;
            if let Some(i) = spatial_neighbor(server, a, f, dir) {
                focus_index(server, i);
            }
        }
        Action::MoveWindow(dir) if n > 0 => {
            let a = active_workspace(server);
            let f = server.workspaces[a].focused;
            let tl = server.workspaces[a].windows[f];
            if (*tl).floating && !(*tl).fullscreen {
                // A floating window has no tiling position to swap; nudge it
                // instead (keyboard-only move until interactive drag lands).
                nudge_floating(server, tl, dir);
            } else if let Some(i) = spatial_neighbor(server, a, f, dir) {
                server.workspaces[a].windows.swap(f, i);
                server.workspaces[a].focused = i;
                refresh(server);
            }
        }
        Action::MoveFocus(_) | Action::MoveWindow(_) => {}
        Action::Fullscreen if n > 0 => {
            let a = active_workspace(server);
            let ws = &server.workspaces[a];
            if let Some(&tl) = ws.windows.get(ws.focused) {
                set_fullscreen(server, tl, !(*tl).fullscreen);
            }
        }
        Action::Fullscreen => {}
        Action::ToggleFloating if n > 0 => {
            let a = active_workspace(server);
            let ws = &server.workspaces[a];
            if let Some(&tl) = ws.windows.get(ws.focused) {
                set_floating(server, tl, !(*tl).floating);
            }
        }
        Action::ToggleFloating => {}
        Action::Workspace(ws) => switch_workspace(server, ws),
        Action::MoveToWorkspace(ws) => move_to_workspace(server, ws),
    }
    true
}
