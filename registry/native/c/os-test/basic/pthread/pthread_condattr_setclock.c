/* Test whether a basic pthread_condattr_setclock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_condattr_t attr;
	if ( (errno = pthread_condattr_init(&attr)) )
		err(1, "pthread_condattr_init");
	clockid_t clock_id;
	if ( (errno = pthread_condattr_getclock(&attr, &clock_id)) )
		err(1, "pthread_condattr_getclock");
	if ( clock_id != CLOCK_REALTIME )
		errx(1, "default clock was not realtime");
	if ( (errno = pthread_condattr_setclock(&attr, CLOCK_MONOTONIC)) )
		err(1, "pthread_condattr_setclock");
	if ( (errno = pthread_condattr_getclock(&attr, &clock_id)) )
		err(1, "pthread_condattr_getclock");
	if ( clock_id != CLOCK_MONOTONIC )
		errx(1, "could not set monotonic clock");
	return 0;
}
