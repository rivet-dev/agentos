/*[SPN]*/
/* Test whether a basic posix_spawnattr_getsigdefault invocation works. */

#include <signal.h>
#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	sigset_t set;
	if ( (errno = posix_spawnattr_getsigdefault(&attr, &set)) )
		err(1, "posix_spawnattr_getsigdefault");
	if ( sigismember(&set, SIGINT) )
		errx(1, "default signal set was not empty");
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	if ( (errno = posix_spawnattr_setsigdefault(&attr, &set)) )
		err(1, "posix_spawnattr_setsigdefault");
	sigset_t new_set;
	if ( (errno = posix_spawnattr_getsigdefault(&attr, &new_set)) )
		err(1, "posix_spawnattr_getsigdefault");
	if ( !sigismember(&set, SIGINT) )
		errx(1, "could not set SIGINT to default");
	return 0;
}
