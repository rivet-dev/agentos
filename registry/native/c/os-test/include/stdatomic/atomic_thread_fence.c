#include <stdatomic.h>
#ifdef atomic_thread_fence
#undef atomic_thread_fence
#endif
void (*foo)(memory_order) = atomic_thread_fence;
int main(void) { return 0; }
