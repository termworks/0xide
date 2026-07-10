#define WLR_USE_UNSTABLE
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_xdg_shell.h>

#include "oxide_shim_internal.h"

struct oxide_listener *oxide_xdg_shell_add_new_toplevel(
        struct wlr_xdg_shell *shell, oxide_callback callback, void *userdata) {
    return signal_add(&shell->events.new_toplevel, callback, userdata);
}

struct wlr_scene_tree *oxide_scene_add_xdg_toplevel(struct wlr_scene_tree *tree,
        struct wlr_xdg_toplevel *toplevel) {
    // A scene node that tracks this surface (and its popups) and follows its
    // map/unmap state automatically.
    return wlr_scene_xdg_surface_create(tree, toplevel->base);
}

// Commit listener, routed to Rust. Fires on every commit; Rust filters for
// the initial one (oxide_xdg_initial_commit) and answers it with a configure
// carrying the window's predicted tile size — so the client's very first
// frame is already the right size instead of its own preferred (often huge)
// one. Returned so Rust can remove it on destroy with the others.
struct oxide_listener *oxide_xdg_add_commit(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata) {
    return signal_add(&toplevel->base->surface->events.commit, callback, userdata);
}

// True only for the client's very first commit — the one the compositor must
// answer with a configure (or the client never maps).
bool oxide_xdg_initial_commit(struct wlr_xdg_toplevel *toplevel) {
    return toplevel->base->initial_commit;
}

struct oxide_listener *oxide_xdg_add_map(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata) {
    return signal_add(&toplevel->base->surface->events.map, callback, userdata);
}

struct oxide_listener *oxide_xdg_add_unmap(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata) {
    return signal_add(&toplevel->base->surface->events.unmap, callback, userdata);
}

struct oxide_listener *oxide_xdg_add_destroy(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata) {
    return signal_add(&toplevel->events.destroy, callback, userdata);
}

void oxide_scene_tree_set_position(struct wlr_scene_tree *tree, int x, int y) {
    wlr_scene_node_set_position(&tree->node, x, y);
}

// Destroy a window's scene tree (used to rebuild it from scratch on VT resume,
// where the original node stops presenting its surface after the outputs are
// torn down and recreated).
void oxide_scene_tree_destroy(struct wlr_scene_tree *tree) {
    wlr_scene_node_destroy(&tree->node);
}

void oxide_scene_tree_set_enabled(struct wlr_scene_tree *tree, bool enabled) {
    wlr_scene_node_set_enabled(&tree->node, enabled);
}

// The toplevel's root wlr_surface — what scene hit-testing resolves clicks to
// (via wlr_surface_get_root_surface), so Rust can match a clicked surface
// back to the Toplevel it tracks.
struct wlr_surface *oxide_xdg_toplevel_surface(struct wlr_xdg_toplevel *toplevel) {
    return toplevel->base->surface;
}

// Fires when the client asks to enter OR leave fullscreen (F11 in a browser,
// mpv --fs). The protocol requires the compositor to answer every state
// request with a configure — Rust does that via wlr_xdg_toplevel_set_fullscreen.
struct oxide_listener *oxide_xdg_add_request_fullscreen(
        struct wlr_xdg_toplevel *toplevel, oxide_callback callback,
        void *userdata) {
    return signal_add(&toplevel->events.request_fullscreen, callback, userdata);
}

// What the client currently wants (checked on the request signal and on map).
bool oxide_xdg_toplevel_requested_fullscreen(struct wlr_xdg_toplevel *toplevel) {
    return toplevel->requested.fullscreen;
}

// Move a window's scene tree to another layer tree (normal <-> fullscreen).
void oxide_scene_tree_reparent(struct wlr_scene_tree *tree,
        struct wlr_scene_tree *new_parent) {
    wlr_scene_node_reparent(&tree->node, new_parent);
}

void oxide_focus_toplevel(struct wlr_seat *seat,
        struct wlr_xdg_toplevel *toplevel) {
    struct wlr_surface *surface = toplevel->base->surface;
    struct wlr_keyboard *kb = wlr_seat_get_keyboard(seat);
    if (kb != NULL) {
        wlr_seat_keyboard_notify_enter(seat, surface, kb->keycodes,
                kb->num_keycodes, &kb->modifiers);
    } else {
        wlr_seat_keyboard_notify_enter(seat, surface, NULL, 0, NULL);
    }
}
