/* Test whether a basic clock_settime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec now;
	if ( clock_gettime(CLOCK_MONOTONIC, &now) < 0 )
		err(1, "clock_gettime");
	if ( clock_settime(CLOCK_MONOTONIC, &now) < 0 )
	{
		if ( errno != EINVAL && errno != EPERM )
			err(1, "clock_settime");
	}
	else
		errx(1, "clock_settime CLOCK_MONOTONIC did not fail");
	return 0;
}
