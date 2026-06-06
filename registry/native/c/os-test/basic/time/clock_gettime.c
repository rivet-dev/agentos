/* Test whether a basic clock_gettime invocation works. */

#include <stdint.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec now;
	if ( clock_gettime(CLOCK_MONOTONIC, &now) < 0 )
		err(1, "clock_gettime");
	if ( now.tv_sec < 0 )
		errx(1, "clock_gettime seconds are negative (%ji.%09li)",
		     (intmax_t) now.tv_sec, now.tv_nsec);
	if ( now.tv_nsec < 0 || 1000000000 <= now.tv_nsec )
		errx(1, "clock_gettime nanoseconds are out of range (%ji.%09li)",
		     (intmax_t) now.tv_sec, now.tv_nsec);
	return 0;
}
