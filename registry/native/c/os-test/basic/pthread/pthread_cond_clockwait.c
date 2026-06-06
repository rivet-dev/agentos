/* Test whether a basic pthread_cond_clockwait invocation works. */

#include <pthread.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	pthread_cond_t cnd;
	if ( (errno = pthread_cond_init(&cnd, NULL)) )
		err(1, "pthread_cond_destroy");
	pthread_mutex_t mtx;
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	if ( (errno = pthread_mutex_lock(&mtx)) )
		err(1, "pthread_mutex_lock");
	struct timespec short_timeout;
	clock_gettime(CLOCK_MONOTONIC, &short_timeout);
	if ( (errno = pthread_cond_clockwait(&cnd, &mtx, CLOCK_MONOTONIC,
	                                     &short_timeout)) )
	{
		if ( errno != ETIMEDOUT )
			err(1, "pthread_cond_clockwait");
	}
	else
		errx(1, "pthread_cond_clockwait did not time out");
	return 0;
}
