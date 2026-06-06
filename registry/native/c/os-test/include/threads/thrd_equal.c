#include <threads.h>
#ifdef thrd_equal
#undef thrd_equal
#endif
int (*foo)(thrd_t, thrd_t) = thrd_equal;
int main(void) { return 0; }
