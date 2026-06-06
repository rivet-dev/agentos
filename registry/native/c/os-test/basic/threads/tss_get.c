/* Test whether a basic tss_get invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	tss_t tss;
	int ret = tss_create(&tss, NULL);
	if ( ret != thrd_success )
		errx(1, "tss_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( tss_get(tss) )
		errx(1, "tss_get returned non-null");
	return 0;
}
