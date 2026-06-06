/*[PS]*/
#include <sched.h>
#ifdef sched_getparam
#undef sched_getparam
#endif
int (*foo)(pid_t, struct sched_param *) = sched_getparam;
int main(void) { return 0; }
