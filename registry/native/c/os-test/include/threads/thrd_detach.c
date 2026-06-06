#include <threads.h>
#ifdef thrd_detach
#undef thrd_detach
#endif
int (*foo)(thrd_t) = thrd_detach;
int main(void) { return 0; }
