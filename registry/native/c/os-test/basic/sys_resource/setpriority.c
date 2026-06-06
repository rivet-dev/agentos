/*[XSI]*/
/* Test whether a basic setpriority invocation works. */

#include <sys/resource.h>

#include <limits.h>

#include "../basic.h"

int main(void)
{
	int priority;
	errno = 0;
	if ( ((priority = getpriority(PRIO_PROCESS, 0)) == -1) && errno )
		err(1, "getpriority: PRIO_PROCESS");
	if ( priority < NZERO - 1 )
		priority++;
	if ( setpriority(PRIO_PROCESS, 0, priority) < 0 )
		err(1, "setpriority: a");
	int new_priority;
	if ( ((new_priority = getpriority(PRIO_PROCESS, 0)) == -1) && errno )
		err(1, "second getpriority: PRIO_PROCESS");
	if ( new_priority != priority )
		printf("setpriority did not set priority");
	return 0;
}
