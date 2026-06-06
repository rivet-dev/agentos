/* Test whether a basic pthread_detach invocation works. */

#include <pthread.h>

#include "../basic.h"

static void* start(void* ctx)
{
	return ctx;
}

int main(void)
{
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_detach(thrd)) )
		err(1, "pthread_detach");
	pthread_exit(NULL);
}
