/* Test whether a basic pthread_rwlock_clockwrlock invocation works. */

#include <pthread.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlock_t rwlock;
	if ( (errno = pthread_rwlock_init(&rwlock, NULL)) )
		err(1, "pthread_rwlock_init");
	struct timespec long_timeout;
	clock_gettime(CLOCK_MONOTONIC, &long_timeout);
	long_timeout.tv_sec += 60;
	if ( (errno = pthread_rwlock_clockwrlock(&rwlock, CLOCK_MONOTONIC,
	                                         &long_timeout)) )
		err(1, "pthread_rwlock_clockwrlock");
	struct timespec short_timeout;
	clock_gettime(CLOCK_MONOTONIC, &short_timeout);
	if ( (errno = pthread_rwlock_clockwrlock(&rwlock, CLOCK_MONOTONIC,
	                                         &short_timeout)) )
	{
		if ( errno != ETIMEDOUT && errno != EDEADLK )
			err(1, "pthread_rwlock_clockwrlock");
	}
	else
		errx(1, "pthread_rwlock_clockwrlock did not time out");
	return 0;
}
