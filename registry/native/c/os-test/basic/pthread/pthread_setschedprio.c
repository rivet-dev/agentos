/*[TPS]*/
/* Test whether a basic pthread_setschedprio invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	struct sched_param params;
	int policy;
	int priority = 0;
	if ( (errno = pthread_getschedparam(pthread_self(), &policy, &params)) )
	{
		//if ( errno != EPERM )
			err(1, "pthread_getschedparam");
	}
	else
		priority = params.sched_priority;
	if ( (errno = pthread_setschedprio(pthread_self(), priority)) /*&&
	     errno != EPERM*/ )
		err(1, "pthread_setschedprio");
	return 0;
}
