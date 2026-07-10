#ifndef EDGE_ICU_DATA_H_
#define EDGE_ICU_DATA_H_

#include <string>

// Registers the embedded ICU common-data blob (ubi_icudt78l_dat, produced by
// cmake/embed_binary.py) with ICU via udata_setCommonData(). Without this, the
// linked ICU has only the empty stubdata image and every locale-aware lookup
// fails with U_MISSING_RESOURCE_ERROR -- which is why Intl had to be hand-rolled.
//
// Runs at most once per process (ICU common data is process-global). Safe to
// call from any thread and any number of times. Returns true on success or if
// the data was already active; on failure writes a message to *error_out (when
// non-null) and returns false.
bool EdgeActivateIcuData(std::string* error_out);

#endif  // EDGE_ICU_DATA_H_
