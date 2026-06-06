/* Test whether a basic timespec_get invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec ts = { .tv_sec = -1, .tv_nsec = -1 };
	int result = timespec_get(&ts, TIME_UTC);
	if ( result == 0 )
		errx(1, "timespec_get returned 0");
	if ( result != TIME_UTC )
		errx(1, "timespec_get did not return TIME_UTC");
	if ( ts.tv_sec == -1 && ts.tv_nsec == -1 )
		errx(1, "timespec_get did not output a timestamp");
	if ( ts.tv_nsec < 0 || 1000000000 <= ts.tv_nsec )
		errx(1, "timespec_get timestamp is not canonical");
	return 0;
}
