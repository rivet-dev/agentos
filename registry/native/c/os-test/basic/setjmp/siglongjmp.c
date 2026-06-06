/* Test whether a basic siglongjmp invocation works. */

#include <setjmp.h>
#include <signal.h>
#include <stdbool.h>

#include "../basic.h"

int main(void)
{
	sigset_t set, oldset	;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	volatile bool done;
	jmp_buf buf;
	int ret;

	// Try siglongjmp with restoring signal mask.
	sigprocmask(SIG_BLOCK, &set, NULL);
	done = false;
	ret = sigsetjmp(buf, 1);
	if ( !ret )
	{
		if ( done )
			errx(1, "sigsetjmp returned 0 twice");
		sigprocmask(SIG_UNBLOCK, &set, NULL);
		done = true;
		siglongjmp(buf, 42);
		return 1;
	}
	if ( !done )
		errx(1, "sigsetjmp returned non-zero before zero");
	if ( ret != 42 )
		errx(1, "sigsetjmp() != 42");
	sigprocmask(SIG_SETMASK, NULL, &oldset);
	if ( !sigismember(&oldset, SIGUSR1) )
		errx(1, "siglongjmp did not restore mask");

	// Try siglongjmp without restoring signal mask.
	sigprocmask(SIG_BLOCK, &set, NULL);
	done = false;
	ret = sigsetjmp(buf, 0);
	if ( !ret )
	{
		if ( done )
			errx(1, "siglongjmp did not change 0 to 1");
		sigprocmask(SIG_UNBLOCK, &set, NULL);
		done = true;
		siglongjmp(buf, 0);
		return 1;
	}
	if ( !done )
		errx(1, "sigsetjmp returned non-zero before zero");
	if ( ret != 1 )
		errx(1, "sigsetjmp() != 1");
	sigprocmask(SIG_SETMASK, NULL, &oldset);
	if ( sigismember(&oldset, SIGUSR1) )
		errx(1, "siglongjmp did not preserve mask");

	return 0;
}
