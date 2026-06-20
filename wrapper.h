// Bindgen entry point: everything #included here becomes Rust declarations.
//
// wlroots gates its real API behind WLR_USE_UNSTABLE — without this define the
// headers expand to `#error`. The shim .c sets the same define for itself.
#define WLR_USE_UNSTABLE

#include <wayland-server-core.h>     // wl_display_*, wl_event_loop
#include <wlr/backend.h>             // wlr_backend_autocreate/start/destroy
#include <wlr/render/allocator.h>    // wlr_allocator_autocreate
#include <wlr/render/wlr_renderer.h> // wlr_renderer_autocreate, init_wl_display
#include <wlr/types/wlr_compositor.h>     // wlr_compositor_create
#include <wlr/types/wlr_subcompositor.h>  // wlr_subcompositor_create
#include <wlr/types/wlr_data_device.h>    // wlr_data_device_manager_create
#include <wlr/types/wlr_output.h>    // wlr_output, wlr_output_init_render
#include <wlr/util/log.h>            // wlr_log_*
#include <wlr/version.h>             // WLR_VERSION_STR
