/* Test whether a basic thrd_detach invocation works. */

#include <threads.h>

#include "../basic.h"

static int start(void* ctx)
{
	(void) ctx;
	return 2;
}

int main(void)
{
	thrd_t thrd;
	int ret = thrd_create(&thrd, start, NULL);
	if ( ret != thrd_success )
		errx(1, "thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = thrd_detach(thrd);
	if ( ret != thrd_success )
		errx(1, "thrd_join: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	thrd_exit(1);
}
