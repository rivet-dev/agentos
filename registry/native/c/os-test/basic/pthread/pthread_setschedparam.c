/*[TPS]*/
/* Test whether a basic pthread_setschedparam invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	struct sched_param params;
	int policy;
	if ( (errno = pthread_getschedparam(pthread_self(), &policy, &params)) /*&&
	     errno != EPERM*/ )
		err(1, "pthread_getschedparam");
	if ( (errno = pthread_setschedparam(pthread_self(), policy, &params))/*&&
	     errno != EPERM */ )
		err(1, "pthread_setschedparam");
	return 0;
}
