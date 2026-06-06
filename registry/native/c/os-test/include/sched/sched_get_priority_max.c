/*[PS|TPS]*/
#include <sched.h>
#ifdef sched_get_priority_max
#undef sched_get_priority_max
#endif
int (*foo)(int) = sched_get_priority_max;
int main(void) { return 0; }
