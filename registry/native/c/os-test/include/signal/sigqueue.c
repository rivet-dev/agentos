#include <signal.h>
#ifdef sigqueue
#undef sigqueue
#endif
int (*foo)(pid_t, int, union sigval) = sigqueue;
int main(void) { return 0; }
