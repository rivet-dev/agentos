/*[TSS]*/
/* Test whether a basic pthread_attr_setstacksize invocation works. */

#include <limits.h>
#include <pthread.h>

#include "../basic.h"

static void* start(void* ctx)
{
	return ctx;
}

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	size_t size = PTHREAD_STACK_MIN;
	if ( (errno = pthread_attr_setstacksize(&attr, size)) )
		err(1, "pthread_attr_setstack");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, &attr, start, NULL)) )
		err(1, "pthread_create");
	pthread_exit(NULL);
}
