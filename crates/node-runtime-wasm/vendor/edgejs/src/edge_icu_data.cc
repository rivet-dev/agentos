#include "edge_icu_data.h"

#include <cstddef>
#include <mutex>
#include <string>

// ICU is built with U_DISABLE_RENAMING=1 (see cmake/EdgeICU.cmake), so consumers
// must match to avoid referencing versioned symbols (udata_setCommonData_78).
#define U_DISABLE_RENAMING 1
#include <unicode/udata.h>
#include <unicode/utypes.h>

// The embedded ICU common-data blob. cmake/embed_binary.py emits it as a raw
// (already bz2-decompressed) byte array plus its length, linked via
// edge_icu_embedded_data on both the native and WASIX targets.
extern "C" {
extern const unsigned char ubi_icudt78l_dat[];
extern const std::size_t ubi_icudt78l_dat_len;
}

namespace {
std::once_flag g_icu_data_once;
bool g_icu_data_ok = false;
std::string g_icu_data_error;
}  // namespace

bool EdgeActivateIcuData(std::string* error_out) {
  std::call_once(g_icu_data_once, []() {
    // Silence the unused-symbol warning while keeping the length referenced so
    // the linker retains the blob; udata_setCommonData reads the length from the
    // ICU data header itself.
    (void)ubi_icudt78l_dat_len;
    UErrorCode status = U_ZERO_ERROR;
    udata_setCommonData(reinterpret_cast<const void*>(ubi_icudt78l_dat), &status);
    if (U_FAILURE(status)) {
      g_icu_data_ok = false;
      g_icu_data_error = u_errorName(status);
      return;
    }
    g_icu_data_ok = true;
  });

  if (!g_icu_data_ok && error_out != nullptr) {
    *error_out = "Failed to activate embedded ICU data: " + g_icu_data_error;
  }
  return g_icu_data_ok;
}
