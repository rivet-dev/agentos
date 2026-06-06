/* Test whether a basic pthread_equal invocation works. */

#include <pthread.h>

#include "../basic.h"

static void* start(void* ctx)
{
	return ctx;
}

int main(void)
{
	pthread_t self = pthread_self();
	if ( !pthread_equal(self, self) )
		errx(1, "pthread_self is not thrd_equal");
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( pthread_equal(self, thrd) )
		errx(1, "pthread_create returned pthread_self");
	if ( !pthread_equal(thrd, thrd) )
		errx(1, "thrd is not thrd");
	return 0;
}
