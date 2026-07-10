#ifndef EDGE_TRACE_H_
#define EDGE_TRACE_H_

#include <cstdlib>

#if !defined(EDGE_ENABLE_TRACE_DIAGNOSTICS)
#if !defined(NDEBUG)
#define EDGE_ENABLE_TRACE_DIAGNOSTICS 1
#else
#define EDGE_ENABLE_TRACE_DIAGNOSTICS 0
#endif
#endif

#if EDGE_ENABLE_TRACE_DIAGNOSTICS
#define EDGE_TRACE_ENABLED(name) (std::getenv(name) != nullptr)
#else
#define EDGE_TRACE_ENABLED(name) false
#endif

#endif  // EDGE_TRACE_H_
