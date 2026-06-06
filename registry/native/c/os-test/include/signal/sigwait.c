#include <signal.h>
#ifdef sigwait
#undef sigwait
#endif
int (*foo)(const sigset_t *restrict, int *restrict) = sigwait;
int main(void) { return 0; }
