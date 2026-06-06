/*[PS]*/
/* Test whether a basic sched_getscheduler invocation works. */

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
	if ( policy != SCHED_OTHER )
		err(1, "policy is not SCHED_OTHER");
	return 0;
}
