/* Test whether a basic timer_create invocation works. */

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
	return 0;
}
