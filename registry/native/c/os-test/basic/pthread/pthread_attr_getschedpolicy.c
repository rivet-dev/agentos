/*[TPS]*/
/* Test whether a basic pthread_attr_getschedpolicy invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	int policy;
	if ( (errno = pthread_attr_getschedpolicy(&attr, &policy)) )
		err(1, "pthread_attr_getschedpolicy");
	if ( policy != SCHED_OTHER )
		errx(1, "default policy was not SCHED_OTHER");
	return 0;
}
