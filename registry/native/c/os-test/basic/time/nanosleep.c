/* Test whether a basic nanosleep invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct timespec delay = { .tv_sec = 0, .tv_nsec = 1 };
	if ( nanosleep(&delay, NULL) < 0 )
		err(1, "nanosleep");
	return 0;
}
