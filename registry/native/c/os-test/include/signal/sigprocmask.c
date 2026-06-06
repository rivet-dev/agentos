#include <signal.h>
#ifdef sigprocmask
#undef sigprocmask
#endif
int (*foo)(int, const sigset_t *restrict, sigset_t *restrict) = sigprocmask;
int main(void) { return 0; }
