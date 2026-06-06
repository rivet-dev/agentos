/*[PS|TPS]*/
#include <sched.h>
#ifdef sched_rr_get_interval
#undef sched_rr_get_interval
#endif
int (*foo)(pid_t, struct timespec *) = sched_rr_get_interval;
int main(void) { return 0; }
