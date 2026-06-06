/* Test whether a basic pthread_attr_setdetachstate invocation works. */

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
	if ( (errno = pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED)) )
		err(1, "pthread_attr_setdetachstate detached");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, &attr, start, NULL)) )
		err(1, "pthread_create");
	pthread_exit(NULL);
}
