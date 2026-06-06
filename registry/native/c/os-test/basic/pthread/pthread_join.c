/* Test whether a basic pthread_join invocation works. */

#include <pthread.h>
#include <stdint.h>

#include "../basic.h"

static void* start(void* ctx)
{
	(void) ctx;
	return (void*) (uintptr_t) 42;
}

int main(void)
{
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, NULL)) )
		err(1, "pthread_create");
	void* code;
	if ( (errno = pthread_join(thrd, &code)) )
		err(1, "pthread_join");
	if ( (uintptr_t) code != 42 )
		errx(1, "pthread_join was not 42 (got %d)", code);
	return 0;
}
