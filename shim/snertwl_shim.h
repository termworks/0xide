#ifndef SNERTWL_SHIM_H
#define SNERTWL_SHIM_H

// Opaque to Rust; full definitions live in the wlroots headers / shim .c.
struct wlr_backend;
struct wlr_output;
struct snertwl_listener;

// Generic event callback handed to Rust: (userdata, signal-data).
typedef void (*snertwl_callback)(void *userdata, void *data);

// --- toolchain / logging ---------------------------------------------------
const char *snertwl_wlroots_version(void);
void snertwl_log_init(void);

// --- listener glue ---------------------------------------------------------
// Subscribe a Rust callback to a wlroots signal. The returned listener is
// heap-allocated and stays put (required: wlroots links it into a list).
struct snertwl_listener *snertwl_backend_add_new_output(
        struct wlr_backend *backend, snertwl_callback callback, void *userdata);
struct snertwl_listener *snertwl_output_add_frame(
        struct wlr_output *output, snertwl_callback callback, void *userdata);

// --- output helpers --------------------------------------------------------
// Enable the output (owns the wlr_output_state init/commit/finish dance).
void snertwl_output_enable(struct wlr_output *output);
// Paint the whole output a solid (r,g,b) and commit it.
void snertwl_output_render_clear(struct wlr_output *output,
        float r, float g, float b);

#endif // SNERTWL_SHIM_H
