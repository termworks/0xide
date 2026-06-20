#define WLR_USE_UNSTABLE
#include <wlr/util/log.h>
#include <wlr/version.h>

#include "snertwl_shim.h"

const char *snertwl_wlroots_version(void) {
    return WLR_VERSION_STR;
}

void snertwl_log_init(void) {
    // Initialize wlroots' logger at debug verbosity with its default stderr
    // sink (NULL callback). Done in C so the wlr_log_importance enum stays
    // native — this is exactly the kind of glue the shim exists to hold.
    wlr_log_init(WLR_DEBUG, NULL);
}
