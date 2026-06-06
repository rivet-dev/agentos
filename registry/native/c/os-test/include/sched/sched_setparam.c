/*[PS]*/
#include <sched.h>
#ifdef sched_setparam
#undef sched_setparam
#endif
int (*foo)(pid_t, const struct sched_param *) = sched_setparam;
int main(void) { return 0; }
