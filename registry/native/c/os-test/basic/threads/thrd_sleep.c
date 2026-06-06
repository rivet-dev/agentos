/* Test whether a basic thrd_sleep invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	struct timespec delay = { .tv_sec = 0, .tv_nsec = 1 };
	struct timespec remaining = { .tv_sec = 1, .tv_nsec = 2 };
	if ( thrd_sleep(&delay, &remaining) < 0 )
		err(1, "thrd_sleep");
	return 0;
}
