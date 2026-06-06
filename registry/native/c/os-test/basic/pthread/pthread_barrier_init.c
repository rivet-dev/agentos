/* Test whether a basic pthread_barrier_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_barrierattr_t attr;
	if ( (errno = pthread_barrierattr_init(&attr)) )
		err(1, "pthread_barrierattr_init");
	pthread_barrier_t barrier;
	if ( (errno = pthread_barrier_init(&barrier, &attr, 2)) )
		err(1, "pthread_barrier_init");
	return 0;
}
