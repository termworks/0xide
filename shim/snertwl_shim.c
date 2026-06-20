#define WLR_USE_UNSTABLE
#include <stdlib.h>
#include <time.h>
#include <wayland-server.h>
#include <xkbcommon/xkbcommon.h>
#include <wlr/backend.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
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

// When a toplevel maps, give it keyboard focus so typing reaches it.
struct snertwl_mapctx {
    struct wlr_seat *seat;
    struct wlr_surface *surface;
};

static void handle_toplevel_map(void *userdata, void *data) {
    (void)data;
    struct snertwl_mapctx *ctx = userdata;
    struct wlr_keyboard *kb = wlr_seat_get_keyboard(ctx->seat);
    if (kb != NULL) {
        wlr_seat_keyboard_notify_enter(ctx->seat, ctx->surface, kb->keycodes,
                kb->num_keycodes, &kb->modifiers);
    } else {
        wlr_seat_keyboard_notify_enter(ctx->seat, ctx->surface, NULL, 0, NULL);
    }
    wlr_log(WLR_INFO, "snertwl: keyboard focus -> toplevel");
}

void snertwl_scene_add_xdg_toplevel(struct wlr_scene *scene,
        struct wlr_xdg_toplevel *toplevel, struct wlr_seat *seat) {
    // A scene node that tracks this surface (and its popups) and follows its
    // map/unmap state automatically.
    wlr_scene_xdg_surface_create(&scene->tree, toplevel->base);
    // Configure the client on its initial commit so it can map.
    signal_add(&toplevel->base->surface->events.commit, handle_xdg_initial_commit,
            toplevel);
    // Focus it on map.
    struct snertwl_mapctx *ctx = calloc(1, sizeof(*ctx));
    ctx->seat = seat;
    ctx->surface = toplevel->base->surface;
    signal_add(&toplevel->base->surface->events.map, handle_toplevel_map, ctx);
}

// --- seat & input ----------------------------------------------------------

struct wlr_seat *snertwl_seat_create(struct wl_display *display, const char *name) {
    struct wlr_seat *seat = wlr_seat_create(display, name);
    // Advertise input capabilities so clients (e.g. foot) will start.
    wlr_seat_set_capabilities(seat,
            WL_SEAT_CAPABILITY_KEYBOARD | WL_SEAT_CAPABILITY_POINTER);
    return seat;
}

// Per-keyboard context so the key/modifier handlers can reach the seat.
struct snertwl_keyboard {
    struct wlr_seat *seat;
    struct wlr_keyboard *keyboard;
};

static void handle_key(void *userdata, void *data) {
    struct snertwl_keyboard *kb = userdata;
    struct wlr_keyboard_key_event *event = data;
    // Forward to the focused client. (Keybindings will intercept here in Stage 5.)
    wlr_seat_set_keyboard(kb->seat, kb->keyboard);
    wlr_seat_keyboard_notify_key(kb->seat, event->time_msec, event->keycode,
            event->state);
}

static void handle_modifiers(void *userdata, void *data) {
    (void)data;
    struct snertwl_keyboard *kb = userdata;
    wlr_seat_set_keyboard(kb->seat, kb->keyboard);
    wlr_seat_keyboard_notify_modifiers(kb->seat, &kb->keyboard->modifiers);
}

static void seat_add_keyboard(struct wlr_seat *seat,
        struct wlr_input_device *device) {
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
    signal_add(&keyboard->events.key, handle_key, kb);
    signal_add(&keyboard->events.modifiers, handle_modifiers, kb);

    wlr_seat_set_keyboard(seat, keyboard);
    wlr_log(WLR_INFO, "snertwl: keyboard attached");
}

struct snertwl_listener *snertwl_backend_add_new_input(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata) {
    return signal_add(&backend->events.new_input, callback, userdata);
}

void snertwl_seat_handle_new_input(struct wlr_seat *seat,
        struct wlr_input_device *device) {
    switch (device->type) {
    case WLR_INPUT_DEVICE_KEYBOARD:
        seat_add_keyboard(seat, device);
        break;
    default:
        // Pointers/touch/etc. handled in Stage 4b.
        break;
    }
}
