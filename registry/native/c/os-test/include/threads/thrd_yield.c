#include <threads.h>
#ifdef thrd_yield
#undef thrd_yield
#endif
void (*foo)(void) = thrd_yield;
int main(void) { return 0; }
