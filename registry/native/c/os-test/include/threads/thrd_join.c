#include <threads.h>
#ifdef thrd_join
#undef thrd_join
#endif
int (*foo)(thrd_t, int *) = thrd_join;
int main(void) { return 0; }
