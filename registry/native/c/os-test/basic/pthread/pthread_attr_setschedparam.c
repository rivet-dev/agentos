/* Test whether a basic pthread_attr_setschedparam invocation works. */

#include <pthread.h>
#include <sched.h>

#include "../basic.h"

int main(void)
{
	int policy;
	struct sched_param param;
	if ( (errno = pthread_getschedparam(pthread_self(), &policy, &param)) )
		err(1, "pthread_getschedparam");
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	if ( (errno = pthread_attr_setschedparam(&attr, &param)) )
		err(1, "pthread_attr_setschedparam");
	return 0;
}
