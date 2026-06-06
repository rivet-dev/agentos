/*[PS|TPS]*/
/* Test whether a basic sched_get_priority_min invocation works. */

#include <sched.h>

#include "../basic.h"

int main(void)
{
	int min = sched_get_priority_min(SCHED_RR);
	if ( min < 0 )
		err(1, "sched_get_priority_min");
	return 0;
}
