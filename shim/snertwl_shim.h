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
// Wire a new input device into the seat. Keyboards get an xkb keymap and have
// their key/modifier events forwarded; other device types wait for Stage 4b.
void snertwl_seat_handle_new_input(struct wlr_seat *seat,
        struct wlr_input_device *device);

#endif // SNERTWL_SHIM_H
