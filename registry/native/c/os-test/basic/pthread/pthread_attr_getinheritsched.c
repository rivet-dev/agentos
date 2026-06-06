/*[TPS]*/
/* Test whether a basic pthread_attr_getinheritsched invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	int inherit;
	if ( (errno = pthread_attr_getinheritsched(&attr, &inherit)) )
		err(1, "pthread_attr_getinheritsched");
	if ( inherit != PTHREAD_INHERIT_SCHED )
		errx(1, "default was not inheriting sched");
	return 0;
}
