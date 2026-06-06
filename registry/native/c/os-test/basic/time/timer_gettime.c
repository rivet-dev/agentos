/* Test whether a basic timer_gettime invocation works. */

#include <signal.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	struct sigevent event =
	{
		.sigev_notify = SIGEV_SIGNAL,
		.sigev_signo = SIGUSR1,
		.sigev_value = { .sival_int = 42 },
	};
	timer_t timer;
	if ( timer_create(CLOCK_MONOTONIC, &event, &timer) < 0 )
		err(1, "timer_create");
	struct itimerspec its;
	if ( timer_gettime(timer, &its) < 0 )
		err(1, "timer_gettime");
	if ( its.it_interval.tv_sec ||
	     its.it_interval.tv_nsec ||
	     its.it_value.tv_sec ||
	     its.it_value.tv_nsec )
		errx(1, "timer_gettime did not return zero itimespec");
	return 0;
}
