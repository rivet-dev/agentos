/*[PS]*/
#include <sched.h>
#ifdef sched_getscheduler
#undef sched_getscheduler
#endif
int (*foo)(pid_t) = sched_getscheduler;
int main(void) { return 0; }
