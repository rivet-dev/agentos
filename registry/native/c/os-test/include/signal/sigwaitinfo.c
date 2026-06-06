#include <signal.h>
#ifdef sigwaitinfo
#undef sigwaitinfo
#endif
int (*foo)(const sigset_t *restrict, siginfo_t *restrict) = sigwaitinfo;
int main(void) { return 0; }
