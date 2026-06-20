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
// Add a toplevel to the scene graph and arrange its initial configure so the
// client can map. Reaches into wlr_xdg_surface fields, hence C-side.
void snertwl_scene_add_xdg_toplevel(struct wlr_scene *scene,
        struct wlr_xdg_toplevel *toplevel);

// --- seat (minimal) --------------------------------------------------------
// Create the wl_seat global and advertise keyboard+pointer so clients will
// start. Actual input device wiring / focus comes in Stage 4.
void snertwl_seat_create(struct wl_display *display, const char *name);

#endif // SNERTWL_SHIM_H
