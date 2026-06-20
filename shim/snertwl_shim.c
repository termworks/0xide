#define WLR_USE_UNSTABLE
#include <stdlib.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/render/pass.h>
#include <wlr/types/wlr_output.h>
#include <wlr/util/box.h>
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

// --- output helpers --------------------------------------------------------

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

void snertwl_output_render_clear(struct wlr_output *output,
        float r, float g, float b) {
    struct wlr_output_state state;
    wlr_output_state_init(&state);

    // Acquires a buffer from the output's swapchain and attaches it to `state`.
    struct wlr_render_pass *pass =
        wlr_output_begin_render_pass(output, &state, NULL);
    if (pass == NULL) {
        wlr_output_state_finish(&state);
        return;
    }

    // One full-output rectangle in BLEND_MODE_NONE = a clear to a solid color.
    struct wlr_render_rect_options rect = {
        .box = {.x = 0, .y = 0, .width = output->width, .height = output->height},
        .color = {.r = r, .g = g, .b = b, .a = 1.0f},
        .blend_mode = WLR_RENDER_BLEND_MODE_NONE,
    };
    wlr_render_pass_add_rect(pass, &rect);

    wlr_render_pass_submit(pass);
    wlr_output_commit_state(output, &state);
    wlr_output_state_finish(&state);
}
