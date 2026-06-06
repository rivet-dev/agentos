/*[SPN]*/
/* Test whether a basic posix_spawnattr_getsigmask invocation works. */

#include <signal.h>
#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	// The default signal mask is unspecified.
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	if ( (errno = posix_spawnattr_setsigmask(&attr, &set)) )
		err(1, "posix_spawnattr_setsigmask");
	sigset_t new_set;
	if ( (errno = posix_spawnattr_getsigmask(&attr, &new_set)) )
		err(1, "posix_spawnattr_getsigmask");
	if ( !sigismember(&set, SIGINT) )
		errx(1, "could not mask SIGINT");
	if ( sigismember(&set, SIGTERM) )
		errx(1, "SIGTERM was unexpectedly masked");
	return 0;
}
