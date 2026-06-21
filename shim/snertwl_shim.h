#ifndef SNERTWL_SHIM_H
#define SNERTWL_SHIM_H

#include <stdbool.h>
#include <stdint.h>

// Opaque to Rust; full definitions live in the wlroots headers / shim .c.
struct wl_display;
struct wl_event_loop;
struct wlr_backend;
struct wlr_output;
struct wlr_scene;
struct wlr_scene_output;
struct wlr_scene_tree;
struct wlr_scene_rect;
struct wlr_xdg_shell;
struct wlr_xdg_toplevel;
struct wlr_seat;
struct wlr_input_device;
struct wlr_cursor;
struct wlr_output_layout;
struct wlr_session;
struct snertwl_listener;

// Generic event callback handed to Rust: (userdata, signal-data).
typedef void (*snertwl_callback)(void *userdata, void *data);

// Key callback: returns true if the keysym was consumed as a binding (and so
// must NOT be forwarded to the focused client).
typedef bool (*snertwl_key_callback)(void *userdata, uint32_t keysym,
        uint32_t modifiers);

// --- toolchain / logging ---------------------------------------------------
const char *snertwl_wlroots_version(void);
void snertwl_log_init(void);

// Resolve a key name (case-insensitive, e.g. "Return"/"q"/"1") to an xkb keysym
// for the config parser. Returns 0 for an unknown name.
uint32_t snertwl_keysym_from_name(const char *name);

// Terminate the display loop on SIGINT/SIGTERM (graceful shutdown).
void snertwl_setup_signals(struct wl_event_loop *loop, struct wl_display *display);

// Switch to virtual terminal `vt` (1-based); no-op if `session` is NULL.
void snertwl_session_change_vt(struct wlr_session *session, unsigned vt);
// Subscribe to session active changes (VT switch away/back). NULL if no session.
struct snertwl_listener *snertwl_session_add_active(struct wlr_session *session,
        snertwl_callback callback, void *userdata);
// True while the session owns the VT (false while switched away).
bool snertwl_session_is_active(struct wlr_session *session);

// --- listener glue ---------------------------------------------------------
struct snertwl_listener *snertwl_backend_add_new_output(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata);
struct snertwl_listener *snertwl_output_add_frame(
        struct wlr_output *output, snertwl_callback callback, void *userdata);
// Fires when an output is destroyed (unplug, or VT-switch seat disable).
struct snertwl_listener *snertwl_output_add_destroy(
        struct wlr_output *output, snertwl_callback callback, void *userdata);

// --- output / scene helpers ------------------------------------------------
// Enable the output (owns the wlr_output_state init/commit/finish dance).
void snertwl_output_enable(struct wlr_output *output);
// Add a solid-color background rectangle, sized to `output`, positioned at the
// output's (x, y) in the layout so multiple outputs don't overlap at the origin.
// Returns the rect so Rust can destroy it when the output goes away.
struct wlr_scene_rect *snertwl_scene_add_output_background(struct wlr_scene *scene,
        struct wlr_output *output, int x, int y, float r, float g, float b);
// Destroy a background rect created above.
void snertwl_scene_rect_destroy(struct wlr_scene_rect *rect);
// Enable/disable a background rect (toggle to force a full-output repaint).
void snertwl_scene_rect_set_enabled(struct wlr_scene_rect *rect, bool enabled);
// Read an output's layout box (position + pixel size) for per-output tiling.
void snertwl_output_layout_get_box(struct wlr_output_layout *layout,
        struct wlr_output *output, int *x, int *y, int *width, int *height);
// The output currently under the cursor (NULL if none).
struct wlr_output *snertwl_output_at_cursor(struct wlr_cursor *cursor,
        struct wlr_output_layout *layout);
// Render + present one frame for this scene output (owns the timespec/clock).
void snertwl_scene_output_render(struct wlr_scene_output *scene_output);
// Request a `frame` event once the output is ready (kicks the first paint).
void snertwl_output_schedule_frame(struct wlr_output *output);

// --- xdg-shell (app windows) ----------------------------------------------
struct snertwl_listener *snertwl_xdg_shell_add_new_toplevel(
        struct wlr_xdg_shell *shell, snertwl_callback callback, void *userdata);
// Add a toplevel to the scene graph. Returns the scene tree node so Rust can
// position it. (Use snertwl_xdg_add_commit for the initial-configure listener.)
struct wlr_scene_tree *snertwl_scene_add_xdg_toplevel(struct wlr_scene *scene,
        struct wlr_xdg_toplevel *toplevel);

// Register the initial-commit handler (lets the client map). Returns the
// listener so Rust can remove it on destroy with the others.
struct snertwl_listener *snertwl_xdg_add_commit(struct wlr_xdg_toplevel *toplevel);

// Unsubscribe and free a listener returned by one of the add helpers.
void snertwl_listener_remove(struct snertwl_listener *listener);

// Lifecycle listeners on a toplevel (Rust drives window tracking & layout).
struct snertwl_listener *snertwl_xdg_add_map(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata);
struct snertwl_listener *snertwl_xdg_add_unmap(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata);
struct snertwl_listener *snertwl_xdg_add_destroy(struct wlr_xdg_toplevel *toplevel,
        snertwl_callback callback, void *userdata);

// Layout helpers: move a window's scene node; give a window keyboard focus;
// read an output's pixel size. (All touch wlroots struct internals.)
void snertwl_scene_tree_set_position(struct wlr_scene_tree *tree, int x, int y);
void snertwl_scene_tree_set_enabled(struct wlr_scene_tree *tree, bool enabled);
// Destroy a window's scene tree (to rebuild it on VT resume).
void snertwl_scene_tree_destroy(struct wlr_scene_tree *tree);
void snertwl_focus_toplevel(struct wlr_seat *seat,
        struct wlr_xdg_toplevel *toplevel);
void snertwl_output_get_size(struct wlr_output *output, int *width, int *height);

// --- seat & input ----------------------------------------------------------
// Create the wl_seat global; returns the seat so Rust can wire input/focus.
struct wlr_seat *snertwl_seat_create(struct wl_display *display, const char *name);
// Subscribe to the backend's new_input signal (data = wlr_input_device).
struct snertwl_listener *snertwl_backend_add_new_input(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata);
// Wire a new input device: keyboards get an xkb keymap + event forwarding
// (key presses are offered to `key_callback` first), pointers get attached to
// the cursor. Other device types are ignored.
void snertwl_handle_new_input(struct wlr_seat *seat, struct wlr_cursor *cursor,
        struct wlr_input_device *device, snertwl_key_callback key_callback,
        void *key_userdata);

// Create a cursor over the output layout, route its events through scene
// hit-testing to the seat, and show a default cursor image. Returns the cursor
// so Rust can attach pointer devices to it.
struct wlr_cursor *snertwl_cursor_setup(struct wlr_output_layout *layout,
        struct wlr_scene *scene, struct wlr_seat *seat);

#endif // SNERTWL_SHIM_H
