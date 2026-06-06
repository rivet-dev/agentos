#include <threads.h>
#ifdef thrd_current
#undef thrd_current
#endif
thrd_t (*foo)(void) = thrd_current;
int main(void) { return 0; }
