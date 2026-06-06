#include <signal.h>
#ifdef sigismember
#undef sigismember
#endif
int (*foo)(const sigset_t *, int) = sigismember;
int main(void) { return 0; }
