#include <threads.h>
#ifdef thrd_create
#undef thrd_create
#endif
int (*foo)(thrd_t *, thrd_start_t, void *) = thrd_create;
int main(void) { return 0; }
