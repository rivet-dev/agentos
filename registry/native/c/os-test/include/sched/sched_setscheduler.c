/*[PS]*/
#include <sched.h>
#ifdef sched_setscheduler
#undef sched_setscheduler
#endif
int (*foo)(pid_t, int, const struct sched_param *) = sched_setscheduler;
int main(void) { return 0; }
