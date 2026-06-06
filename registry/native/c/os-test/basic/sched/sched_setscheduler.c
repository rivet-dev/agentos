/*[PS]*/
/* Test whether a basic sched_setscheduler invocation works. */

#include <sched.h>

#include "../basic.h"

int main(void)
{
	int policy = sched_getscheduler(0);
	if ( policy < 0 )
	{
		if ( errno == EPERM )
			exit(0);
		err(1, "sched_getscheduler");
	}
	struct sched_param params;
	if ( sched_getparam(0, &params) < 0 )
	{
		if ( errno == EPERM )
			exit(0);
		err(1, "sched_getparam");
	}
	if ( sched_setscheduler(0, policy, &params) < 0 )
	{
		if ( errno == EPERM )
			exit(0);
		err(1, "sched_setscheduler");
	}
	return 0;
}
