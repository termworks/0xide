// Bindgen entry point: everything #included here becomes Rust declarations.
//
// wlroots gates its real API behind WLR_USE_UNSTABLE — without this define the
// headers expand to `#error`. build.rs passes the same flag to clang; defining
// it here too keeps the header self-contained and identical to what the shim sees.
#define WLR_USE_UNSTABLE

#include <wlr/util/log.h>
#include <wlr/version.h>
