#define WLR_USE_UNSTABLE
#include <signal.h>
#include <stdlib.h>
#include <time.h>
#include <wayland-server.h>
#include <xkbcommon/xkbcommon.h>
#include <wlr/backend.h>
#include <wlr/backend/session.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/types/wlr_layer_shell_v1.h>
#include <wlr/types/wlr_xdg_decoration_v1.h>
#include <wlr/util/log.h>
#include <wlr/version.h>

#include "oxide_shim.h"

const char *oxide_wlroots_version(void) {
    return WLR_VERSION_STR;
}

void oxide_log_init(void) {
    // Debug verbosity, default stderr sink. Done in C so the enum stays native.
    wlr_log_init(WLR_DEBUG, NULL);
}

// Resolve a key name from the config (e.g. "Return", "q", "1") to an xkb keysym,
// case-insensitively. Returns 0 (XKB_KEY_NoSymbol) for an unknown name. We match
// bindings on level-0 keysyms, so case-insensitive lookup gives the unshifted
// form (e.g. "Q" -> lowercase q), exactly what handle_key reports.
uint32_t oxide_keysym_from_name(const char *name) {
    return xkb_keysym_from_name(name, XKB_KEYSYM_CASE_INSENSITIVE);
}

static int handle_signal(int sig, void *data) {
    (void)sig;
    wl_display_terminate(data); // unwinds wl_display_run -> graceful shutdown
    return 0;
}

void oxide_setup_signals(struct wl_event_loop *loop, struct wl_display *display) {
    // Handled via the event loop's signalfd, so it's safe (not an async signal).
    wl_event_loop_add_signal(loop, SIGINT, handle_signal, display);
    wl_event_loop_add_signal(loop, SIGTERM, handle_signal, display);
}

// --- session / VT ----------------------------------------------------------

// Switch to virtual terminal `vt` (1-based). No-op when there's no session
// (e.g. running nested, where autocreate hands back a NULL session).
void oxide_session_change_vt(struct wlr_session *session, unsigned vt) {
    if (session != NULL) {
        wlr_session_change_vt(session, vt);
    }
}

// True if the session currently owns the VT (false while switched away).
bool oxide_session_is_active(struct wlr_session *session) {
    return session != NULL && session->active;
}

// --- listener glue ---------------------------------------------------------
//
// wlroots delivers every event through wl_signal/wl_listener: you embed a
// wl_listener in your own struct, attach it to a signal, and on fire recover
// your struct from the listener pointer via wl_container_of (offsetof math).
// We wrap that intrusive pattern once and expose a plain (userdata, data)
// callback so Rust never touches the linked list or the pointer arithmetic.

struct oxide_listener {
    struct wl_listener listener; // must stay put once added to a signal
    oxide_callback callback;   // Rust function to invoke
    void *userdata;              // opaque pointer Rust handed us
};

static void oxide_listener_notify(struct wl_listener *listener, void *data) {
    struct oxide_listener *l = wl_container_of(listener, l, listener);
    l->callback(l->userdata, data);
}

static struct oxide_listener *signal_add(struct wl_signal *signal,
        oxide_callback callback, void *userdata) {
    struct oxide_listener *l = calloc(1, sizeof(*l));
    l->listener.notify = oxide_listener_notify;
    l->callback = callback;
    l->userdata = userdata;
    wl_signal_add(signal, &l->listener);
    return l;
}

struct oxide_listener *oxide_backend_add_new_output(
        struct wlr_backend *backend, oxide_callback callback, void *userdata) {
    return signal_add(&backend->events.new_output, callback, userdata);
}

// Subscribe to the session active signal (fires on every VT switch, away and
// back). The handler uses oxide_session_is_active to tell direction. No-op
// (NULL) when there's no session, e.g. nested.
struct oxide_listener *oxide_session_add_active(struct wlr_session *session,
        oxide_callback callback, void *userdata) {
    if (session == NULL) {
        return NULL;
    }
    return signal_add(&session->events.active, callback, userdata);
}

struct oxide_listener *oxide_output_add_frame(
        struct wlr_output *output, oxide_callback callback, void *userdata) {
    return signal_add(&output->events.frame, callback, userdata);
}

// Output destroy fires when a monitor is removed — including when logind
// disables the seat on a VT switch (the DRM backend tears the output down).
// Rust uses this to remove its frame listener (else wlr_output_finish asserts)
// and drop the output from its list. `data` is the wlr_output.
struct oxide_listener *oxide_output_add_destroy(
        struct wlr_output *output, oxide_callback callback, void *userdata) {
    return signal_add(&output->events.destroy, callback, userdata);
}

// --- output / scene helpers ------------------------------------------------

void oxide_output_enable(struct wlr_output *output) {
    struct wlr_output_state state;
    wlr_output_state_init(&state);
    wlr_output_state_set_enabled(&state, true);

    // Windowed backends (nested Wayland/X11) expose no modes; only real
    // displays do. Pick the preferred one when present.
    struct wlr_output_mode *mode = wlr_output_preferred_mode(output);
    if (mode != NULL) {
        wlr_output_state_set_mode(&state, mode);
    }

    wlr_output_commit_state(output, &state);
    wlr_output_state_finish(&state);
}

// Create an ordered child tree directly under the scene root. Creation order
// is paint order (later = on top) — this is how we get correct layer-shell
// z-ordering without touching wlroots' internals.
struct wlr_scene_tree *oxide_scene_add_layer_tree(struct wlr_scene *scene) {
    return wlr_scene_tree_create(&scene->tree);
}

struct wlr_scene_rect *oxide_scene_add_output_background(struct wlr_scene_tree *tree,
        struct wlr_output *output, int x, int y, float r, float g, float b) {
    const float color[4] = {r, g, b, 1.0f};
    struct wlr_scene_rect *rect =
            wlr_scene_rect_create(tree, output->width, output->height, color);
    // Scene nodes share one coordinate space; place this output's background at
    // the output's position in the layout so multiple monitors don't overlap.
    wlr_scene_node_set_position(&rect->node, x, y);
    return rect;
}

// Remove a background rectangle (when its output is destroyed).
void oxide_scene_rect_destroy(struct wlr_scene_rect *rect) {
    wlr_scene_node_destroy(&rect->node);
}

// Enable/disable a background rect. Toggling it off then on damages the whole
// output region (the rect spans the output), which forces a full re-present —
// used on VT resume so idle windows, which produce no damage of their own, get
// flipped back to the screen instead of staying black.
void oxide_scene_rect_set_enabled(struct wlr_scene_rect *rect, bool enabled) {
    wlr_scene_node_set_enabled(&rect->node, enabled);
}

// Which output is under the cursor right now (NULL if none). Lets Rust target
// the monitor the mouse is on for new windows / workspace switches.
struct wlr_output *oxide_output_at_cursor(struct wlr_cursor *cursor,
        struct wlr_output_layout *layout) {
    return wlr_output_layout_output_at(layout, cursor->x, cursor->y);
}

// Read an output's box (position + size) in layout coordinates, so Rust can tile
// windows within the correct monitor. (Touches wlr_box internals.)
void oxide_output_layout_get_box(struct wlr_output_layout *layout,
        struct wlr_output *output, int *x, int *y, int *width, int *height) {
    struct wlr_box box;
    wlr_output_layout_get_box(layout, output, &box);
    *x = box.x;
    *y = box.y;
    *width = box.width;
    *height = box.height;
}

// Ask the output to emit a `frame` event when it's ready to draw. Used to kick
// the first paint of a (re)created output without rendering before it's ready —
// the frame handler then does a full repaint, so resumed windows reappear.
void oxide_output_schedule_frame(struct wlr_output *output) {
    wlr_output_schedule_frame(output);
}

void oxide_scene_output_render(struct wlr_scene_output *scene_output) {
    // The scene does the damage-tracked render pass internally, then we tell
    // clients their frame was shown so they can produce the next one.
    wlr_scene_output_commit(scene_output, NULL);
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    wlr_scene_output_send_frame_done(scene_output, &now);
}

// --- xdg-shell (app windows) ----------------------------------------------

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

// Unsubscribe and free a listener. Each per-window listener must be removed
// before its object is destroyed (wlroots asserts an empty destroy list).
void oxide_listener_remove(struct oxide_listener *l) {
    wl_list_remove(&l->listener.link);
    free(l);
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

void oxide_output_get_size(struct wlr_output *output, int *width, int *height) {
    *width = output->width;
    *height = output->height;
}

// --- layer-shell (bars, panels, wallpaper) ----------------------------------
//
// The global itself is created directly from Rust via the bindgen binding for
// wlr_layer_shell_v1_create (same pattern as wlr_xdg_shell_create) — no shim
// wrapper needed for a plain creation call.

struct oxide_listener *oxide_layer_shell_add_new_surface(
        struct wlr_layer_shell_v1 *shell, oxide_callback callback, void *userdata) {
    return signal_add(&shell->events.new_surface, callback, userdata);
}

// The output may be NULL if the client didn't request a specific one; Rust
// must assign one before returning from the new_surface handler.
struct wlr_output *oxide_layer_surface_output(struct wlr_layer_surface_v1 *ls) {
    return ls->output;
}

void oxide_layer_surface_set_output(struct wlr_layer_surface_v1 *ls,
        struct wlr_output *output) {
    ls->output = output;
}

// Requested z-layer (0=background..3=overlay, zwlr_layer_shell_v1_layer). Set
// directly by the get_layer_surface request, so it's already valid when
// new_surface fires.
uint32_t oxide_layer_surface_layer(struct wlr_layer_surface_v1 *ls) {
    return ls->pending.layer;
}

struct wlr_scene_layer_surface_v1 *oxide_scene_layer_surface_create(
        struct wlr_scene_tree *tree, struct wlr_layer_surface_v1 *ls) {
    return wlr_scene_layer_surface_v1_create(tree, ls);
}

// Position/size `scene_ls` per its anchors+margins within the output box
// (fx,fy,fw,fh), and shrink the usable box (ux,uy,uw,uh, in/out) by its
// exclusive zone. wlr_scene_layer_surface_v1_configure also sends the
// layer_surface.configure event that lets the client map.
void oxide_scene_layer_surface_configure(struct wlr_scene_layer_surface_v1 *scene_ls,
        int fx, int fy, int fw, int fh, int *ux, int *uy, int *uw, int *uh) {
    struct wlr_box full = {.x = fx, .y = fy, .width = fw, .height = fh};
    struct wlr_box usable = {.x = *ux, .y = *uy, .width = *uw, .height = *uh};
    wlr_scene_layer_surface_v1_configure(scene_ls, &full, &usable);
    *ux = usable.x;
    *uy = usable.y;
    *uw = usable.width;
    *uh = usable.height;
}

struct oxide_listener *oxide_layer_surface_add_commit(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata) {
    return signal_add(&ls->surface->events.commit, callback, userdata);
}

struct oxide_listener *oxide_layer_surface_add_map(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata) {
    return signal_add(&ls->surface->events.map, callback, userdata);
}

struct oxide_listener *oxide_layer_surface_add_unmap(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata) {
    return signal_add(&ls->surface->events.unmap, callback, userdata);
}

struct oxide_listener *oxide_layer_surface_add_destroy(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata) {
    return signal_add(&ls->events.destroy, callback, userdata);
}

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

// --- seat & input ----------------------------------------------------------

struct wlr_seat *oxide_seat_create(struct wl_display *display, const char *name) {
    struct wlr_seat *seat = wlr_seat_create(display, name);
    // Advertise input capabilities so clients (e.g. foot) will start.
    wlr_seat_set_capabilities(seat,
            WL_SEAT_CAPABILITY_KEYBOARD | WL_SEAT_CAPABILITY_POINTER);
    return seat;
}

// Per-keyboard context so the key/modifier handlers can reach the seat and the
// Rust keybinding callback. We track our listeners so we can remove them when
// the device is destroyed (e.g. on VT switch, when logind pauses input) —
// otherwise wlroots asserts the keyboard's signal lists aren't empty.
struct oxide_keyboard {
    struct wlr_seat *seat;
    struct wlr_keyboard *keyboard;
    oxide_key_callback key_callback;
    void *key_userdata;
    struct oxide_listener *key_listener;
    struct oxide_listener *mod_listener;
    struct oxide_listener *destroy_listener;
};

static void handle_key(void *userdata, void *data) {
    struct oxide_keyboard *kb = userdata;
    struct wlr_keyboard_key_event *event = data;

    // Offer the press to Rust as a possible keybinding first. wlroots keycodes
    // are offset by 8 from xkb keycodes.
    bool handled = false;
    if (event->state == WL_KEYBOARD_KEY_STATE_PRESSED && kb->key_callback != NULL) {
        uint32_t keycode = event->keycode + 8;
        // Match bindings on the layout level-0 (unshifted) keysym, so e.g.
        // Mod+Shift+1 reads as '1' (+Shift modifier), not the shifted '!'.
        xkb_layout_index_t layout =
                xkb_state_key_get_layout(kb->keyboard->xkb_state, keycode);
        const xkb_keysym_t *syms;
        int nsyms = xkb_keymap_key_get_syms_by_level(kb->keyboard->keymap,
                keycode, layout, 0, &syms);
        uint32_t modifiers = wlr_keyboard_get_modifiers(kb->keyboard);
        for (int i = 0; i < nsyms; i++) {
            if (kb->key_callback(kb->key_userdata, syms[i], modifiers)) {
                handled = true;
            }
        }
    }

    // Unhandled keys go to the focused client.
    if (!handled) {
        wlr_seat_set_keyboard(kb->seat, kb->keyboard);
        wlr_seat_keyboard_notify_key(kb->seat, event->time_msec, event->keycode,
                event->state);
    }
}

static void handle_modifiers(void *userdata, void *data) {
    (void)data;
    struct oxide_keyboard *kb = userdata;
    wlr_seat_set_keyboard(kb->seat, kb->keyboard);
    wlr_seat_keyboard_notify_modifiers(kb->seat, &kb->keyboard->modifiers);
}

// The input device is going away (unplugged, or paused on a VT switch). Detach
// our listeners before wlroots tears the keyboard down, then free our context.
static void handle_keyboard_destroy(void *userdata, void *data) {
    (void)data;
    struct oxide_keyboard *kb = userdata;
    oxide_listener_remove(kb->key_listener);
    oxide_listener_remove(kb->mod_listener);
    oxide_listener_remove(kb->destroy_listener);
    free(kb);
    wlr_log(WLR_INFO, "0xide: keyboard removed");
}

static void seat_add_keyboard(struct wlr_seat *seat,
        struct wlr_input_device *device, oxide_key_callback key_callback,
        void *key_userdata) {
    struct wlr_keyboard *keyboard = wlr_keyboard_from_input_device(device);

    // Compile scancodes -> keysyms with the default (locale/us) layout.
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_keymap *keymap =
            xkb_keymap_new_from_names(context, NULL, XKB_KEYMAP_COMPILE_NO_FLAGS);
    wlr_keyboard_set_keymap(keyboard, keymap);
    xkb_keymap_unref(keymap);
    xkb_context_unref(context);
    wlr_keyboard_set_repeat_info(keyboard, 25, 600);

    struct oxide_keyboard *kb = calloc(1, sizeof(*kb));
    kb->seat = seat;
    kb->keyboard = keyboard;
    kb->key_callback = key_callback;
    kb->key_userdata = key_userdata;
    kb->key_listener = signal_add(&keyboard->events.key, handle_key, kb);
    kb->mod_listener = signal_add(&keyboard->events.modifiers, handle_modifiers, kb);
    // Device-level destroy, so we clean up when the keyboard is removed.
    kb->destroy_listener = signal_add(&device->events.destroy, handle_keyboard_destroy, kb);

    wlr_seat_set_keyboard(seat, keyboard);
    wlr_log(WLR_INFO, "0xide: keyboard attached");
}

struct oxide_listener *oxide_backend_add_new_input(
        struct wlr_backend *backend, oxide_callback callback, void *userdata) {
    return signal_add(&backend->events.new_input, callback, userdata);
}

// --- pointer / cursor ------------------------------------------------------

// Bundles everything the cursor event handlers need.
struct oxide_pointer {
    struct wlr_cursor *cursor;
    struct wlr_xcursor_manager *cursor_mgr;
    struct wlr_scene *scene;
    struct wlr_seat *seat;
};

// Find the surface under the cursor (and the surface-local coords), via the
// scene graph. Returns NULL when the cursor is over the bare background.
static struct wlr_surface *surface_at(struct oxide_pointer *p,
        double *sx, double *sy) {
    struct wlr_scene_node *node = wlr_scene_node_at(&p->scene->tree.node,
            p->cursor->x, p->cursor->y, sx, sy);
    if (node == NULL || node->type != WLR_SCENE_NODE_BUFFER) {
        return NULL;
    }
    struct wlr_scene_surface *scene_surface =
            wlr_scene_surface_try_from_buffer(wlr_scene_buffer_from_node(node));
    return scene_surface ? scene_surface->surface : NULL;
}

static void process_motion(struct oxide_pointer *p, uint32_t time) {
    double sx, sy;
    struct wlr_surface *surface = surface_at(p, &sx, &sy);
    if (surface == NULL) {
        // Over the background: show our own cursor, focus nothing.
        wlr_cursor_set_xcursor(p->cursor, p->cursor_mgr, "default");
        wlr_seat_pointer_clear_focus(p->seat);
    } else {
        wlr_seat_pointer_notify_enter(p->seat, surface, sx, sy);
        wlr_seat_pointer_notify_motion(p->seat, time, sx, sy);
    }
}

static void handle_cursor_motion(void *userdata, void *data) {
    struct oxide_pointer *p = userdata;
    struct wlr_pointer_motion_event *e = data;
    wlr_cursor_move(p->cursor, &e->pointer->base, e->delta_x, e->delta_y);
    process_motion(p, e->time_msec);
}

static void handle_cursor_motion_absolute(void *userdata, void *data) {
    struct oxide_pointer *p = userdata;
    struct wlr_pointer_motion_absolute_event *e = data;
    wlr_cursor_warp_absolute(p->cursor, &e->pointer->base, e->x, e->y);
    process_motion(p, e->time_msec);
}

static void handle_cursor_button(void *userdata, void *data) {
    struct oxide_pointer *p = userdata;
    struct wlr_pointer_button_event *e = data;
    wlr_seat_pointer_notify_button(p->seat, e->time_msec, e->button, e->state);
    // Click-to-focus: on press, give keyboard focus to the window under cursor.
    if (e->state == WL_POINTER_BUTTON_STATE_PRESSED) {
        double sx, sy;
        struct wlr_surface *surface = surface_at(p, &sx, &sy);
        struct wlr_keyboard *kb = wlr_seat_get_keyboard(p->seat);
        if (surface != NULL && kb != NULL) {
            wlr_seat_keyboard_notify_enter(p->seat, surface, kb->keycodes,
                    kb->num_keycodes, &kb->modifiers);
        }
    }
}

static void handle_cursor_axis(void *userdata, void *data) {
    struct oxide_pointer *p = userdata;
    struct wlr_pointer_axis_event *e = data;
    wlr_seat_pointer_notify_axis(p->seat, e->time_msec, e->orientation, e->delta,
            e->delta_discrete, e->source, e->relative_direction);
}

static void handle_cursor_frame(void *userdata, void *data) {
    (void)data;
    struct oxide_pointer *p = userdata;
    wlr_seat_pointer_notify_frame(p->seat);
}

struct wlr_cursor *oxide_cursor_setup(struct wlr_output_layout *layout,
        struct wlr_scene *scene, struct wlr_seat *seat) {
    struct wlr_cursor *cursor = wlr_cursor_create();
    wlr_cursor_attach_output_layout(cursor, layout);

    struct wlr_xcursor_manager *cursor_mgr = wlr_xcursor_manager_create(NULL, 24);
    wlr_xcursor_manager_load(cursor_mgr, 1);

    struct oxide_pointer *p = calloc(1, sizeof(*p));
    p->cursor = cursor;
    p->cursor_mgr = cursor_mgr;
    p->scene = scene;
    p->seat = seat;

    signal_add(&cursor->events.motion, handle_cursor_motion, p);
    signal_add(&cursor->events.motion_absolute, handle_cursor_motion_absolute, p);
    signal_add(&cursor->events.button, handle_cursor_button, p);
    signal_add(&cursor->events.axis, handle_cursor_axis, p);
    signal_add(&cursor->events.frame, handle_cursor_frame, p);

    return cursor;
}

void oxide_handle_new_input(struct wlr_seat *seat, struct wlr_cursor *cursor,
        struct wlr_input_device *device, oxide_key_callback key_callback,
        void *key_userdata) {
    switch (device->type) {
    case WLR_INPUT_DEVICE_KEYBOARD:
        seat_add_keyboard(seat, device, key_callback, key_userdata);
        break;
    case WLR_INPUT_DEVICE_POINTER:
        wlr_cursor_attach_input_device(cursor, device);
        wlr_log(WLR_INFO, "0xide: pointer attached");
        break;
    default:
        break;
    }
}
