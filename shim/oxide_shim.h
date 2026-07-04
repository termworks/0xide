#ifndef OXIDE_SHIM_H
#define OXIDE_SHIM_H

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
struct wlr_layer_shell_v1;
struct wlr_layer_surface_v1;
struct wlr_scene_layer_surface_v1;
struct wlr_xdg_decoration_manager_v1;
struct wlr_seat;
struct wlr_input_device;
struct wlr_cursor;
struct wlr_output_layout;
struct wlr_session;
struct oxide_listener;

// Generic event callback handed to Rust: (userdata, signal-data).
typedef void (*oxide_callback)(void *userdata, void *data);

// Key callback: returns true if the keysym was consumed as a binding (and so
// must NOT be forwarded to the focused client).
typedef bool (*oxide_key_callback)(void *userdata, uint32_t keysym,
        uint32_t modifiers);

// --- toolchain / logging ---------------------------------------------------
const char *oxide_wlroots_version(void);
void oxide_log_init(void);

// Resolve a key name (case-insensitive, e.g. "Return"/"q"/"1") to an xkb keysym
// for the config parser. Returns 0 for an unknown name.
uint32_t oxide_keysym_from_name(const char *name);

// Terminate the display loop on SIGINT/SIGTERM (graceful shutdown).
void oxide_setup_signals(struct wl_event_loop *loop, struct wl_display *display);

// Switch to virtual terminal `vt` (1-based); no-op if `session` is NULL.
void oxide_session_change_vt(struct wlr_session *session, unsigned vt);
// Subscribe to session active changes (VT switch away/back). NULL if no session.
struct oxide_listener *oxide_session_add_active(struct wlr_session *session,
        oxide_callback callback, void *userdata);
// True while the session owns the VT (false while switched away).
bool oxide_session_is_active(struct wlr_session *session);

// --- listener glue ---------------------------------------------------------
struct oxide_listener *oxide_backend_add_new_output(
        struct wlr_backend *backend, oxide_callback callback, void *userdata);
struct oxide_listener *oxide_output_add_frame(
        struct wlr_output *output, oxide_callback callback, void *userdata);
// Fires when an output is destroyed (unplug, or VT-switch seat disable).
struct oxide_listener *oxide_output_add_destroy(
        struct wlr_output *output, oxide_callback callback, void *userdata);

// --- output / scene helpers ------------------------------------------------
// Enable the output (owns the wlr_output_state init/commit/finish dance),
// applying `scale` (1.0 if the config doesn't set one).
void oxide_output_enable(struct wlr_output *output, float scale);
// The connector name (e.g. "eDP-1", "HDMI-A-1"), for matching against a
// `monitor = NAME, ...` config entry.
const char *oxide_output_name(struct wlr_output *output);
// Create an ordered child tree directly under the scene root. Rust calls this
// once per z-layer at startup (bg_fallback, layer_bg, layer_bottom, normal,
// layer_top, layer_overlay); creation order is paint order (later = on top).
struct wlr_scene_tree *oxide_scene_add_layer_tree(struct wlr_scene *scene);
// Add a solid-color background rectangle, sized to `output`, positioned at the
// output's (x, y) in the layout so multiple outputs don't overlap at the
// origin, under `tree` (the bg_fallback layer). Returns the rect so Rust can
// destroy it when the output goes away.
struct wlr_scene_rect *oxide_scene_add_output_background(struct wlr_scene_tree *tree,
        struct wlr_output *output, int x, int y, float r, float g, float b);
// Destroy a background rect created above.
void oxide_scene_rect_destroy(struct wlr_scene_rect *rect);
// Enable/disable a background rect (toggle to force a full-output repaint).
void oxide_scene_rect_set_enabled(struct wlr_scene_rect *rect, bool enabled);
// Read an output's layout box (position + pixel size) for per-output tiling.
void oxide_output_layout_get_box(struct wlr_output_layout *layout,
        struct wlr_output *output, int *x, int *y, int *width, int *height);
// The output currently under the cursor (NULL if none).
struct wlr_output *oxide_output_at_cursor(struct wlr_cursor *cursor,
        struct wlr_output_layout *layout);
// Render + present one frame for this scene output (owns the timespec/clock).
void oxide_scene_output_render(struct wlr_scene_output *scene_output);
// Request a `frame` event once the output is ready (kicks the first paint).
void oxide_output_schedule_frame(struct wlr_output *output);

// --- xdg-shell (app windows) ----------------------------------------------
struct oxide_listener *oxide_xdg_shell_add_new_toplevel(
        struct wlr_xdg_shell *shell, oxide_callback callback, void *userdata);
// Add a toplevel to the scene graph, under `tree` (the normal/app-window
// layer). Returns the scene tree node so Rust can position it. (Use
// oxide_xdg_add_commit for the initial-configure listener.)
struct wlr_scene_tree *oxide_scene_add_xdg_toplevel(struct wlr_scene_tree *tree,
        struct wlr_xdg_toplevel *toplevel);

// Register the initial-commit handler (lets the client map). Returns the
// listener so Rust can remove it on destroy with the others.
struct oxide_listener *oxide_xdg_add_commit(struct wlr_xdg_toplevel *toplevel);

// Unsubscribe and free a listener returned by one of the add helpers.
void oxide_listener_remove(struct oxide_listener *listener);

// Lifecycle listeners on a toplevel (Rust drives window tracking & layout).
struct oxide_listener *oxide_xdg_add_map(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata);
struct oxide_listener *oxide_xdg_add_unmap(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata);
struct oxide_listener *oxide_xdg_add_destroy(struct wlr_xdg_toplevel *toplevel,
        oxide_callback callback, void *userdata);

// Layout helpers: move a window's scene node; give a window keyboard focus;
// read an output's pixel size. (All touch wlroots struct internals.)
void oxide_scene_tree_set_position(struct wlr_scene_tree *tree, int x, int y);
void oxide_scene_tree_set_enabled(struct wlr_scene_tree *tree, bool enabled);
// Destroy a window's scene tree (to rebuild it on VT resume).
void oxide_scene_tree_destroy(struct wlr_scene_tree *tree);
void oxide_focus_toplevel(struct wlr_seat *seat,
        struct wlr_xdg_toplevel *toplevel);
void oxide_output_get_size(struct wlr_output *output, int *width, int *height);

// --- layer-shell (bars, panels, wallpaper) ----------------------------------
//
// Mirrors the xdg-shell wiring above: wlroots implements the protocol and a
// scene helper that does anchor/margin math and exclusive-zone bookkeeping;
// we expose that plus lifecycle listeners. Rust owns which tree (z-layer) and
// output each surface lands on.

// The zwlr_layer_shell_v1 global is created directly from Rust via bindgen's
// wlr_layer_shell_v1_create binding (same pattern as wlr_xdg_shell_create).
struct oxide_listener *oxide_layer_shell_add_new_surface(
        struct wlr_layer_shell_v1 *shell, oxide_callback callback, void *userdata);

// NULL if the client didn't request a specific output; Rust must assign one
// before returning from the new_surface handler.
struct wlr_output *oxide_layer_surface_output(struct wlr_layer_surface_v1 *ls);
void oxide_layer_surface_set_output(struct wlr_layer_surface_v1 *ls,
        struct wlr_output *output);
// Requested z-layer: 0=background, 1=bottom, 2=top, 3=overlay
// (zwlr_layer_shell_v1_layer).
uint32_t oxide_layer_surface_layer(struct wlr_layer_surface_v1 *ls);

// Add the layer surface (and its sub-surfaces/popups) to the scene, under the
// tree matching its layer.
struct wlr_scene_layer_surface_v1 *oxide_scene_layer_surface_create(
        struct wlr_scene_tree *tree, struct wlr_layer_surface_v1 *ls);
// Position/size the surface per its anchors+margins within the output box
// (fx,fy,fw,fh), and shrink the usable box (ux,uy,uw,uh, in/out) by its
// exclusive zone. Also sends the layer_surface.configure event to the client.
void oxide_scene_layer_surface_configure(struct wlr_scene_layer_surface_v1 *scene_ls,
        int fx, int fy, int fw, int fh, int *ux, int *uy, int *uw, int *uh);

// Lifecycle listeners on a layer surface (Rust drives arrange + tiling).
struct oxide_listener *oxide_layer_surface_add_commit(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata);
struct oxide_listener *oxide_layer_surface_add_map(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata);
struct oxide_listener *oxide_layer_surface_add_unmap(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata);
struct oxide_listener *oxide_layer_surface_add_destroy(struct wlr_layer_surface_v1 *ls,
        oxide_callback callback, void *userdata);

// --- xdg-decoration (server-side window decoration negotiation) ------------
//
// The manager global is created directly from Rust via bindgen's
// wlr_xdg_decoration_manager_v1_create binding (same pattern as
// wlr_xdg_shell_create) — no shim wrapper needed for a plain creation call.

struct oxide_listener *oxide_xdg_decoration_manager_add_new_toplevel_decoration(
        struct wlr_xdg_decoration_manager_v1 *manager, oxide_callback callback,
        void *userdata);
// Force server-side mode: the client stops drawing its own title bar/border.
// We draw nothing in its place (bare windows), so this is a one-shot,
// fire-and-forget call — no per-decoration state to track or clean up.
void oxide_xdg_toplevel_decoration_set_server_side(void *decoration);

// --- seat & input ----------------------------------------------------------
// Create the wl_seat global; returns the seat so Rust can wire input/focus.
struct wlr_seat *oxide_seat_create(struct wl_display *display, const char *name);
// Subscribe to the backend's new_input signal (data = wlr_input_device).
struct oxide_listener *oxide_backend_add_new_input(
        struct wlr_backend *backend, oxide_callback callback, void *userdata);
// Wire a new input device: keyboards get an xkb keymap + event forwarding
// (key presses are offered to `key_callback` first), pointers get attached to
// the cursor. Other device types are ignored.
void oxide_handle_new_input(struct wlr_seat *seat, struct wlr_cursor *cursor,
        struct wlr_input_device *device, oxide_key_callback key_callback,
        void *key_userdata);

// Create a cursor over the output layout, route its events through scene
// hit-testing to the seat, and show a default cursor image. Returns the cursor
// so Rust can attach pointer devices to it.
struct wlr_cursor *oxide_cursor_setup(struct wlr_output_layout *layout,
        struct wlr_scene *scene, struct wlr_seat *seat);

#endif // OXIDE_SHIM_H
