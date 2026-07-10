#ifndef EDGE_PRECOMPILE_H_
#define EDGE_PRECOMPILE_H_

#include <string>
#include <vector>

#include "node_api.h"

namespace edge_precompile {

// Walks the given files/directories, compiles every eligible CommonJS source
// (.cjs always; .js unless its nearest package.json declares type "module")
// and writes bytecode sidecars next to the sources. Module bodies are never
// executed. Returns the process exit code (0 = all eligible files written,
// 1 = at least one failure).
int RunPrecompile(napi_env env,
                  const std::vector<std::string>& paths,
                  std::string* error_out);

}  // namespace edge_precompile

#endif  // EDGE_PRECOMPILE_H_
