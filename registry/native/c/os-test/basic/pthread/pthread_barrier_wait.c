/* Test whether a basic pthread_barrier_wait invocation works. */

#include <stdbool.h>
#include <pthread.h>

#include "../basic.h"

static pthread_barrier_t barrier;
static bool ran = false;

static void* start(void* ctx)
{
	ran = true;
	if ( (errno = pthread_barrier_wait(&barrier)) &&
	     errno != PTHREAD_BARRIER_SERIAL_THREAD )
		err(1, "thread pthread_barrier_wait");
	return ctx;
}

int main(void)
{
	pthread_barrierattr_t attr;
	if ( (errno = pthread_barrierattr_init(&attr)) )
		err(1, "pthread_barrierattr_init");
	if ( (errno = pthread_barrier_init(&barrier, &attr, 2)) )
		err(1, "pthread_barrier_init");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_barrier_wait(&barrier)) &&
	     errno != PTHREAD_BARRIER_SERIAL_THREAD )
		err(1, "main pthread_barrier_wait");
	if ( !ran )
		errx(1, "thread did not run");
	pthread_join(thread, NULL);
	return 0;
}
