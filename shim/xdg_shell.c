#define WLR_USE_UNSTABLE
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_xdg_shell.h>

#include "oxide_shim_internal.h"

// On the client's very first commit we must answer with a configure, or it
// never maps. Size 0,0 means "client, pick your own size".
static void handle_xdg_initial_commit(void *userdata, void *data) {
    (void)data;
    struct wlr_xdg_toplevel *toplevel = userdata;
    if (toplevel->base->initial_commit) {
        wlr_xdg_toplevel_set_size(toplevel, 0, 0);
    }
}

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

// Configure the client on its initial commit so it can map. Returned so Rust
// can remove it (with the other per-window listeners) on destroy.
struct oxide_listener *oxide_xdg_add_commit(struct wlr_xdg_toplevel *toplevel) {
    return signal_add(&toplevel->base->surface->events.commit,
            handle_xdg_initial_commit, toplevel);
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
