#include <signal.h>
#ifdef sigtimedwait
#undef sigtimedwait
#endif
int (*foo)(const sigset_t *restrict, siginfo_t *restrict, const struct timespec *restrict) = sigtimedwait;
int main(void) { return 0; }
