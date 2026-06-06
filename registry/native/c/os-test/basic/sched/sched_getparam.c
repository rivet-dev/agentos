/*[PS]*/
/* Test whether a basic sched_getparam invocation works. */

#include <sched.h>
#include <unistd.h>

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
	return 0;
}
