/* Test whether a basic clock_nanosleep invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec now;
	if ( clock_gettime(CLOCK_MONOTONIC, &now) < 0 )
		err(1, "clock_gettime");
	if ( clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &now, NULL) < 0 )
		err(1, "clock_nanosleep");
	return 0;
}
