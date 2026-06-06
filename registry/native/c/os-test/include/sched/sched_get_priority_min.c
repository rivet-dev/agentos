/*[PS|TPS]*/
#include <sched.h>
#ifdef sched_get_priority_min
#undef sched_get_priority_min
#endif
int (*foo)(int) = sched_get_priority_min;
int main(void) { return 0; }
