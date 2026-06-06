#include <signal.h>
#ifdef sigsuspend
#undef sigsuspend
#endif
int (*foo)(const sigset_t *) = sigsuspend;
int main(void) { return 0; }
