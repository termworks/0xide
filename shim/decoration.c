#define WLR_USE_UNSTABLE
#include <stdlib.h>
#include <wlr/types/wlr_xdg_decoration_v1.h>

#include "oxide_shim_internal.h"

// --- xdg-decoration (server-side window decoration negotiation) ------------

struct oxide_listener *oxide_xdg_decoration_manager_add_new_toplevel_decoration(
        struct wlr_xdg_decoration_manager_v1 *manager, oxide_callback callback,
        void *userdata) {
    return signal_add(&manager->events.new_toplevel_decoration, callback, userdata);
}

// Most clients create the decoration object before ever committing their
// surface; at that point the underlying xdg_surface isn't "initialized" yet,
// and wlr_xdg_toplevel_decoration_v1_set_mode() (via
// wlr_xdg_surface_schedule_configure) asserts on that. So: set the mode right
// away if the surface is already initialized, otherwise wait for its first
// commit. Guards against the toplevel being destroyed before ever committing.
struct oxide_decoration_pending {
    struct wlr_xdg_toplevel_decoration_v1 *decoration;
    struct oxide_listener *commit_listener;
    struct oxide_listener *destroy_listener;
};

static void decoration_pending_free(struct oxide_decoration_pending *p) {
    oxide_listener_remove(p->commit_listener);
    oxide_listener_remove(p->destroy_listener);
    free(p);
}

static void handle_decoration_initial_commit(void *userdata, void *data) {
    (void)data;
    struct oxide_decoration_pending *p = userdata;
    if (p->decoration->toplevel->base->initial_commit) {
        wlr_xdg_toplevel_decoration_v1_set_mode(p->decoration,
                WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
        decoration_pending_free(p);
    }
}

static void handle_decoration_destroy_before_commit(void *userdata, void *data) {
    (void)data;
    decoration_pending_free(userdata);
}

void oxide_xdg_toplevel_decoration_set_server_side(void *decoration) {
    struct wlr_xdg_toplevel_decoration_v1 *deco = decoration;
    if (deco->toplevel->base->initialized) {
        wlr_xdg_toplevel_decoration_v1_set_mode(deco,
                WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
        return;
    }
    struct oxide_decoration_pending *p = calloc(1, sizeof(*p));
    p->decoration = deco;
    p->commit_listener = signal_add(&deco->toplevel->base->surface->events.commit,
            handle_decoration_initial_commit, p);
    p->destroy_listener = signal_add(&deco->events.destroy,
            handle_decoration_destroy_before_commit, p);
}
