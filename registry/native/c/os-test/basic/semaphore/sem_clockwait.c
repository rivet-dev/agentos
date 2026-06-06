/* Test whether a basic sem_clockwait invocation works. */

#include <errno.h>
#include <semaphore.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	sem_t sem;
	if ( sem_init(&sem, 0, 1) < 0 )
		err(1, "sem_init");
	struct timespec long_timeout;
	clock_gettime(CLOCK_MONOTONIC, &long_timeout);
	long_timeout.tv_sec += 60;
	if ( sem_clockwait(&sem, CLOCK_MONOTONIC, &long_timeout) < 0 )
		err(1, "sem_clockwait");
	struct timespec short_timeout;
	clock_gettime(CLOCK_MONOTONIC, &short_timeout);
	if ( sem_clockwait(&sem, CLOCK_MONOTONIC, &short_timeout) < 0 )
	{
		if ( errno != ETIMEDOUT )
			err(1, "sem_clockwait");
	}
	else
		errx(1, "sem_clockwait did not time out");
	return 0;
}
