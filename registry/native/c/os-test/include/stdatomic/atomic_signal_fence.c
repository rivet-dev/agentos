#include <stdatomic.h>
#ifdef atomic_signal_fence
#undef atomic_signal_fence
#endif
void (*foo)(memory_order) = atomic_signal_fence;
int main(void) { return 0; }
