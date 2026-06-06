/* Test whether a basic thrd_equal invocation works. */

#include <threads.h>

#include "../basic.h"

static int start(void* ctx)
{
	(void) ctx;
	return 0;
}

int main(void)
{
	thrd_t self = thrd_current();
	if ( !thrd_equal(self, self) )
		errx(1, "thrd_current is not thrd_equal");
	thrd_t thrd;
	int ret = thrd_create(&thrd, start, NULL);
	if ( ret != thrd_success )
		errx(1, "thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( thrd_equal(self, thrd) )
		errx(1, "thrd_create returned thrd_current");
	if ( !thrd_equal(thrd, thrd) )
		errx(1, "thrd is not thrd");
	return 0;
}
