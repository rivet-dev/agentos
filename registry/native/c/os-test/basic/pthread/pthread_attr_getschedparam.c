/* Test whether a basic pthread_attr_getschedparam invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	struct sched_param param;
	if ( (errno = pthread_attr_getschedparam(&attr, &param)) )
		err(1, "pthread_attr_getschedparam");
	return 0;
}
