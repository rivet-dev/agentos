/* Test whether a basic clock_getres invocation works. */

#include <stdint.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec res;
	if ( clock_getres(CLOCK_MONOTONIC, &res) < 0 )
		err(1, "clock_getres");
	if ( res.tv_sec < 0 )
		errx(1, "clock_getres seconds are negative (%ji.%09li)",
		     (intmax_t) res.tv_sec, res.tv_nsec);
	if ( res.tv_nsec < 0 || 1000000000 <= res.tv_nsec )
		errx(1, "clock_getres nanoseconds are out of range (%ji.%09li)",
		     (intmax_t) res.tv_sec, res.tv_nsec);
	if ( !res.tv_sec && !res.tv_nsec )
		errx(1, "clock_getres has zero resolution (%ji.%09li)",
		     (intmax_t) res.tv_sec, res.tv_nsec);
	return 0;
}
