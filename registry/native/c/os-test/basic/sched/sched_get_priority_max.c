/*[PS|TPS]*/
/* Test whether a basic sched_get_priority_max invocation works. */

#include <sched.h>

#include "../basic.h"

int main(void)
{
	int max = sched_get_priority_max(SCHED_RR);
	if ( max < 0 )
		err(1, "sched_get_priority_max");
	int min = sched_get_priority_min(SCHED_RR);
	if ( min < 0 )
		err(1, "sched_get_priority_min");
	// 2.8.4 Process Scheduling "Conforming implementations shall provide a
	// priority range of at least 32 priorities for this policy."
	if ( max - min < 31 )
		errx(1, "SCHED_RR range %d-%d has less than 32 values", min, max);
	return 0;
}
