/*[PS|TPS]*/
/* Test whether a basic sched_rr_get_interval invocation works. */

#include <sched.h>

#include <stdint.h>
#include <stdio.h>

#include "../basic.h"

int main(void)
{
	struct timespec ts;
	if ( sched_rr_get_interval(0, &ts) < 0 )
		err(1, "sched_rr_get_interval");
	return 0;
}
