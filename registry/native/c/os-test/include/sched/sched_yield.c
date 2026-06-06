#include <sched.h>
#ifdef sched_yield
#undef sched_yield
#endif
int (*foo)(void) = sched_yield;
int main(void) { return 0; }
