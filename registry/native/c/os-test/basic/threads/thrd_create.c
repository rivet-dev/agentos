/* Test whether a basic thrd_create invocation works. */

#include <threads.h>

#include "../basic.h"

static volatile int running = 1;
static int magic = 42;

static int start(void* ctx)
{
	if ( *((int*) ctx) != 42 )
		errx(1, "start had wrong parameter");
	exit(0);
	running = 0;
}

int main(void)
{
	thrd_t thrd;
	int ret = thrd_create(&thrd, start, &magic);
	if ( ret != thrd_success )
		errx(1, "thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	struct timespec delay = { .tv_sec = 0 };
	while ( running )
		thrd_sleep(&delay, NULL);
	return 1;
}
