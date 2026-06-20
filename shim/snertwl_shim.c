#define WLR_USE_UNSTABLE
#include <stdlib.h>
#include <time.h>
#include <wayland-server.h>
#include <xkbcommon/xkbcommon.h>
#include <wlr/backend.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/util/log.h>
#include <wlr/version.h>

#include "snertwl_shim.h"

const char *snertwl_wlroots_version(void) {
    return WLR_VERSION_STR;
}

void snertwl_log_init(void) {
    // Debug verbosity, default stderr sink. Done in C so the enum stays native.
    wlr_log_init(WLR_DEBUG, NULL);
}

// --- listener glue ---------------------------------------------------------
//
// wlroots delivers every event through wl_signal/wl_listener: you embed a
// wl_listener in your own struct, attach it to a signal, and on fire recover
// your struct from the listener pointer via wl_container_of (offsetof math).
// We wrap that intrusive pattern once and expose a plain (userdata, data)
// callback so Rust never touches the linked list or the pointer arithmetic.

struct snertwl_listener {
    struct wl_listener listener; // must stay put once added to a signal
    snertwl_callback callback;   // Rust function to invoke
    void *userdata;              // opaque pointer Rust handed us
};

static void snertwl_listener_notify(struct wl_listener *listener, void *data) {
    struct snertwl_listener *l = wl_container_of(listener, l, listener);
    l->callback(l->userdata, data);
}

static struct snertwl_listener *signal_add(struct wl_signal *signal,
        snertwl_callback callback, void *userdata) {
    struct snertwl_listener *l = calloc(1, sizeof(*l));
    l->listener.notify = snertwl_listener_notify;
    l->callback = callback;
    l->userdata = userdata;
    wl_signal_add(signal, &l->listener);
    return l;
}

struct snertwl_listener *snertwl_backend_add_new_output(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata) {
    return signal_add(&backend->events.new_output, callback, userdata);
}

struct snertwl_listener *snertwl_output_add_frame(
        struct wlr_output *output, snertwl_callback callback, void *userdata) {
    return signal_add(&output->events.frame, callback, userdata);
}

// --- output / scene helpers ------------------------------------------------

void snertwl_output_enable(struct wlr_output *output) {
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

void snertwl_scene_add_output_background(struct wlr_scene *scene,
        struct wlr_output *output, float r, float g, float b) {
    const float color[4] = {r, g, b, 1.0f};
    wlr_scene_rect_create(&scene->tree, output->width, output->height, color);
}

void snertwl_scene_output_render(struct wlr_scene_output *scene_output) {
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

struct snertwl_listener *snertwl_xdg_shell_add_new_toplevel(
        struct wlr_xdg_shell *shell, snertwl_callback callback, void *userdata) {
    return signal_add(&shell->events.new_toplevel, callback, userdata);
}

struct wlr_scene_tree *snertwl_scene_add_xdg_toplevel(struct wlr_scene *scene,
        struct wlr_xdg_toplevel *toplevel) {
    // A scene node that tracks this surface (and its popups) and follows its
    // map/unmap state automatically.
    struct wlr_scene_tree *tree =
            wlr_scene_xdg_surface_create(&scene->tree, toplevel->base);
    // Configure the client on its initial commit so it can map.
    signal_add(&toplevel->base->surface->events.commit, handle_xdg_initial_commit,
            toplevel);
    return tree;
}

struct snertwl_listener *snertwl_xdg_add_map(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata) {
    return signal_add(&toplevel->base->surface->events.map, callback, userdata);
}

struct snertwl_listener *snertwl_xdg_add_unmap(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata) {
    return signal_add(&toplevel->base->surface->events.unmap, callback, userdata);
}

struct snertwl_listener *snertwl_xdg_add_destroy(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata) {
    return signal_add(&toplevel->events.destroy, callback, userdata);
}

void snertwl_scene_tree_set_position(struct wlr_scene_tree *tree, int x, int y) {
    wlr_scene_node_set_position(&tree->node, x, y);
}

void snertwl_focus_toplevel(struct wlr_seat *seat,
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

void snertwl_output_get_size(struct wlr_output *output, int *width, int *height) {
    *width = output->width;
    *height = output->height;
}

// --- seat & input ----------------------------------------------------------

struct wlr_seat *snertwl_seat_create(struct wl_display *display, const char *name) {
    struct wlr_seat *seat = wlr_seat_create(display, name);
    // Advertise input capabilities so clients (e.g. foot) will start.
    wlr_seat_set_capabilities(seat,
            WL_SEAT_CAPABILITY_KEYBOARD | WL_SEAT_CAPABILITY_POINTER);
    return seat;
}

// Per-keyboard context so the key/modifier handlers can reach the seat and the
// Rust keybinding callback.
struct snertwl_keyboard {
    struct wlr_seat *seat;
    struct wlr_keyboard *keyboard;
    snertwl_key_callback key_callback;
    void *key_userdata;
};

static void handle_key(void *userdata, void *data) {
    struct snertwl_keyboard *kb = userdata;
    struct wlr_keyboard_key_event *event = data;

    // Offer the press to Rust as a possible keybinding first. wlroots keycodes
    // are offset by 8 from xkb keycodes.
    bool handled = false;
    if (event->state == WL_KEYBOARD_KEY_STATE_PRESSED && kb->key_callback != NULL) {
        const xkb_keysym_t *syms;
        int nsyms = xkb_state_key_get_syms(kb->keyboard->xkb_state,
                event->keycode + 8, &syms);
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
    struct snertwl_keyboard *kb = userdata;
    wlr_seat_set_keyboard(kb->seat, kb->keyboard);
    wlr_seat_keyboard_notify_modifiers(kb->seat, &kb->keyboard->modifiers);
}

static void seat_add_keyboard(struct wlr_seat *seat,
        struct wlr_input_device *device, snertwl_key_callback key_callback,
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

    struct snertwl_keyboard *kb = calloc(1, sizeof(*kb));
    kb->seat = seat;
    kb->keyboard = keyboard;
    kb->key_callback = key_callback;
    kb->key_userdata = key_userdata;
    signal_add(&keyboard->events.key, handle_key, kb);
    signal_add(&keyboard->events.modifiers, handle_modifiers, kb);

    wlr_seat_set_keyboard(seat, keyboard);
    wlr_log(WLR_INFO, "snertwl: keyboard attached");
}

struct snertwl_listener *snertwl_backend_add_new_input(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata) {
    return signal_add(&backend->events.new_input, callback, userdata);
}

// --- pointer / cursor ------------------------------------------------------

// Bundles everything the cursor event handlers need.
struct snertwl_pointer {
    struct wlr_cursor *cursor;
    struct wlr_xcursor_manager *cursor_mgr;
    struct wlr_scene *scene;
    struct wlr_seat *seat;
};

// Find the surface under the cursor (and the surface-local coords), via the
// scene graph. Returns NULL when the cursor is over the bare background.
static struct wlr_surface *surface_at(struct snertwl_pointer *p,
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

static void process_motion(struct snertwl_pointer *p, uint32_t time) {
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
    struct snertwl_pointer *p = userdata;
    struct wlr_pointer_motion_event *e = data;
    wlr_cursor_move(p->cursor, &e->pointer->base, e->delta_x, e->delta_y);
    process_motion(p, e->time_msec);
}

static void handle_cursor_motion_absolute(void *userdata, void *data) {
    struct snertwl_pointer *p = userdata;
    struct wlr_pointer_motion_absolute_event *e = data;
    wlr_cursor_warp_absolute(p->cursor, &e->pointer->base, e->x, e->y);
    process_motion(p, e->time_msec);
}

static void handle_cursor_button(void *userdata, void *data) {
    struct snertwl_pointer *p = userdata;
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
    struct snertwl_pointer *p = userdata;
    struct wlr_pointer_axis_event *e = data;
    wlr_seat_pointer_notify_axis(p->seat, e->time_msec, e->orientation, e->delta,
            e->delta_discrete, e->source, e->relative_direction);
}

static void handle_cursor_frame(void *userdata, void *data) {
    (void)data;
    struct snertwl_pointer *p = userdata;
    wlr_seat_pointer_notify_frame(p->seat);
}

struct wlr_cursor *snertwl_cursor_setup(struct wlr_output_layout *layout,
        struct wlr_scene *scene, struct wlr_seat *seat) {
    struct wlr_cursor *cursor = wlr_cursor_create();
    wlr_cursor_attach_output_layout(cursor, layout);

    struct wlr_xcursor_manager *cursor_mgr = wlr_xcursor_manager_create(NULL, 24);
    wlr_xcursor_manager_load(cursor_mgr, 1);

    struct snertwl_pointer *p = calloc(1, sizeof(*p));
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

void snertwl_handle_new_input(struct wlr_seat *seat, struct wlr_cursor *cursor,
        struct wlr_input_device *device, snertwl_key_callback key_callback,
        void *key_userdata) {
    switch (device->type) {
    case WLR_INPUT_DEVICE_KEYBOARD:
        seat_add_keyboard(seat, device, key_callback, key_userdata);
        break;
    case WLR_INPUT_DEVICE_POINTER:
        wlr_cursor_attach_input_device(cursor, device);
        wlr_log(WLR_INFO, "snertwl: pointer attached");
        break;
    default:
        break;
    }
}
