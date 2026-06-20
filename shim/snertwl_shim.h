#ifndef SNERTWL_SHIM_H
#define SNERTWL_SHIM_H

// The wlroots version snertwl was compiled against, e.g. "0.19.3".
const char *snertwl_wlroots_version(void);

// Initialize wlroots logging (debug verbosity, default stderr sink).
void snertwl_log_init(void);

#endif // SNERTWL_SHIM_H
