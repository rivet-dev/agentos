/* Test whether a basic pthread_mutex_clocklock invocation works. */

#include <pthread.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	pthread_mutex_t mtx;
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	struct timespec long_timeout;
	clock_gettime(CLOCK_MONOTONIC, &long_timeout);
	long_timeout.tv_sec += 60;
	if ( (errno = pthread_mutex_timedlock(&mtx, &long_timeout)) )
		err(1, "pthread_mutex_timedlock");
	struct timespec short_timeout;
	clock_gettime(CLOCK_MONOTONIC, &short_timeout);
	if ( (errno = pthread_mutex_clocklock(&mtx, CLOCK_MONOTONIC,
	                                      &short_timeout)) )
	{
		if ( errno != ETIMEDOUT )
			err(1, "pthread_mutex_clocklock");
	}
	else
		errx(1, "pthread_mutex_clocklock did not time out");
	return 0;
}
