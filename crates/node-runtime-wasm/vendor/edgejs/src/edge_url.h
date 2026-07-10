#ifndef EDGE_URL_H_
#define EDGE_URL_H_

#include <string>
#include <string_view>

#include "node_api.h"

napi_value EdgeInstallUrlBinding(napi_env env);
napi_value EdgeInstallUrlPatternBinding(napi_env env);

namespace edge_url {

// pathToFileURL equivalent for absolute paths — byte-identical to the JS
// loader's url.pathToFileURL() href (same percent-encoding via ada). Returns
// an empty string when the input does not parse as a file URL.
std::string PathToFileURLString(std::string_view absolute_path);

}  // namespace edge_url

#endif  // EDGE_URL_H_
