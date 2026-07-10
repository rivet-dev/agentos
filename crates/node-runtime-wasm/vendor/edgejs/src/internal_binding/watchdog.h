#ifndef EDGE_INTERNAL_BINDING_WATCHDOG_H_
#define EDGE_INTERNAL_BINDING_WATCHDOG_H_

namespace internal_binding {

bool StartSigintWatchdog();
bool StopSigintWatchdog();
bool WatchdogHasPendingSigint();

}  // namespace internal_binding

#endif  // EDGE_INTERNAL_BINDING_WATCHDOG_H_
