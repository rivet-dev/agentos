/* Test whether a basic pthread_testcancel invocation works. */

#include <pthread.h>
#include <stdbool.h>

#include "../basic.h"

static void* start(void* ctx)
{
	(void) ctx;
	while ( true )
		pthread_testcancel();
	return ctx;
}

int main(void)
{
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_cancel(thread)) )
		err(1, "pthread_cancel");
	void* result;
	if ( (errno = pthread_join(thread, &result)) )
		err(1, "pthread_cancel");
	if ( result != PTHREAD_CANCELED )
		errx(1, "pthread_join() != PTHREAD_CANCELED");
	return 0;
}
