/* Test whether a basic cnd_signal invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	cnd_t cnd;
	int ret = cnd_init(&cnd);
	if ( ret != thrd_success )
		errx(1, "cnd_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = cnd_signal(&cnd);
	if ( ret != thrd_success )
		errx(1, "cnd_signal: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	return 0;
}
