#ifndef EDGE_INTL_H_
#define EDGE_INTL_H_

#include <string>

#include "node_api.h"

// Installs the ICU-backed Intl constructors onto globalThis.Intl. Assumes the
// embedded ICU data has already been activated (see EdgeActivateIcuData); each
// constructor is backed by the linked edge_icu_i18n library. Adds to an existing
// Intl object if present, otherwise creates one. Returns false and writes to
// *error_out (when non-null) on a hard failure.
//
// This is the real ECMA-402 surface; it replaced the hand-rolled en-US
// Intl.DateTimeFormat stub that predated it.
bool EdgeInstallIntl(napi_env env, std::string* error_out);

#endif  // EDGE_INTL_H_
