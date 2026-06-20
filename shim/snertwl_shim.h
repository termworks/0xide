#ifndef SNERTWL_SHIM_H
#define SNERTWL_SHIM_H

// Opaque to Rust; full definitions live in the wlroots headers / shim .c.
struct wl_display;
struct wlr_backend;
struct wlr_output;
struct wlr_scene;
struct wlr_scene_output;
struct wlr_xdg_shell;
struct wlr_xdg_toplevel;
struct wlr_seat;
struct wlr_input_device;
struct wlr_cursor;
struct wlr_output_layout;
struct snertwl_listener;

// Generic event callback handed to Rust: (userdata, signal-data).
typedef void (*snertwl_callback)(void *userdata, void *data);

// --- toolchain / logging ---------------------------------------------------
const char *snertwl_wlroots_version(void);
void snertwl_log_init(void);

// --- listener glue ---------------------------------------------------------
struct snertwl_listener *snertwl_backend_add_new_output(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata);
struct snertwl_listener *snertwl_output_add_frame(
        struct wlr_output *output, snertwl_callback callback, void *userdata);

// --- output / scene helpers ------------------------------------------------
// Enable the output (owns the wlr_output_state init/commit/finish dance).
void snertwl_output_enable(struct wlr_output *output);
// Add a solid-color background rectangle, sized to `output`, at the scene root.
void snertwl_scene_add_output_background(struct wlr_scene *scene,
        struct wlr_output *output, float r, float g, float b);
// Render + present one frame for this scene output (owns the timespec/clock).
void snertwl_scene_output_render(struct wlr_scene_output *scene_output);

// --- xdg-shell (app windows) ----------------------------------------------
struct snertwl_listener *snertwl_xdg_shell_add_new_toplevel(
        struct wlr_xdg_shell *shell, snertwl_callback callback, void *userdata);
// Add a toplevel to the scene graph, arrange its initial configure, and give
// it keyboard focus when it maps. Reaches into wlr_xdg_surface fields, C-side.
void snertwl_scene_add_xdg_toplevel(struct wlr_scene *scene,
        struct wlr_xdg_toplevel *toplevel, struct wlr_seat *seat);

// --- seat & input ----------------------------------------------------------
// Create the wl_seat global; returns the seat so Rust can wire input/focus.
struct wlr_seat *snertwl_seat_create(struct wl_display *display, const char *name);
// Subscribe to the backend's new_input signal (data = wlr_input_device).
struct snertwl_listener *snertwl_backend_add_new_input(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata);
// Wire a new input device: keyboards get an xkb keymap + event forwarding,
// pointers get attached to the cursor. Other device types are ignored.
void snertwl_handle_new_input(struct wlr_seat *seat, struct wlr_cursor *cursor,
        struct wlr_input_device *device);

// Create a cursor over the output layout, route its events through scene
// hit-testing to the seat, and show a default cursor image. Returns the cursor
// so Rust can attach pointer devices to it.
struct wlr_cursor *snertwl_cursor_setup(struct wlr_output_layout *layout,
        struct wlr_scene *scene, struct wlr_seat *seat);

#endif // SNERTWL_SHIM_H
