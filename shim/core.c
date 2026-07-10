#define WLR_USE_UNSTABLE
#include <signal.h>
#include <stdlib.h>
#include <wayland-server.h>
#include <xkbcommon/xkbcommon.h>
#include <wlr/backend/session.h>
#include <wlr/util/log.h>
#include <wlr/version.h>

#include "oxide_shim_internal.h"

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
    // Auto-reap spawned clients (POSIX: ignoring SIGCHLD reaps children on
    // exit). We never wait() on any child, so without this every closed app
    // would stay a zombie for the compositor session's lifetime.
    signal(SIGCHLD, SIG_IGN);
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

// --- listener glue -----------------------------------------------------

static void oxide_listener_notify(struct wl_listener *listener, void *data) {
    struct oxide_listener *l = wl_container_of(listener, l, listener);
    l->callback(l->userdata, data);
}

struct oxide_listener *signal_add(struct wl_signal *signal,
        oxide_callback callback, void *userdata) {
    struct oxide_listener *l = calloc(1, sizeof(*l));
    l->listener.notify = oxide_listener_notify;
    l->callback = callback;
    l->userdata = userdata;
    wl_signal_add(signal, &l->listener);
    return l;
}

// Unsubscribe and free a listener. Each per-window listener must be removed
// before its object is destroyed (wlroots asserts an empty destroy list).
void oxide_listener_remove(struct oxide_listener *l) {
    wl_list_remove(&l->listener.link);
    free(l);
}
