/*[PS]*/
/* Test whether a basic sched_setparam invocation works. */

#include <sched.h>

#include "../basic.h"

int main(void)
{
	struct sched_param params;
	if ( sched_getparam(0, &params) < 0 )
	{
		if ( errno == EPERM )
			exit(0);
		err(1, "sched_getparam");
	}
	if ( sched_setparam(0, &params) < 0 )
		err(1, "sched_setparam");
	return 0;
}
