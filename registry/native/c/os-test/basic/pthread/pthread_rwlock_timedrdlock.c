/* Test whether a basic pthread_rwlock_timedrdlock invocation works. */

#include <pthread.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlock_t rwlock;
	if ( (errno = pthread_rwlock_init(&rwlock, NULL)) )
		err(1, "pthread_rwlock_init");
	struct timespec long_timeout;
	clock_gettime(CLOCK_REALTIME, &long_timeout);
	long_timeout.tv_sec += 60;
	if ( (errno = pthread_rwlock_timedwrlock(&rwlock, &long_timeout)) )
		err(1, "pthread_rwlock_timedrdlock");
	if ( (errno = pthread_rwlock_unlock(&rwlock)) )
		err(1, "pthread_rwlock_timedrdlock");
	if ( (errno = pthread_rwlock_wrlock(&rwlock)) )
		err(1, "pthread_rwlock_wrlock");
	struct timespec short_timeout;
	clock_gettime(CLOCK_REALTIME, &short_timeout);
	if ( (errno = pthread_rwlock_timedrdlock(&rwlock, &short_timeout)) )
	{
		if ( errno != ETIMEDOUT && errno != EDEADLK )
			err(1, "pthread_rwlock_timedrdlock");
	}
	else
		errx(1, "pthread_rwlock_timedrdlock did not time out");
	return 0;
}
