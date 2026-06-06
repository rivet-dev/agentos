/*[SPN PS]*/
/* Test whether a basic posix_spawnattr_getschedparam invocation works. */

#include <sched.h>
#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	// The default schedparam is unspecified.
	struct sched_param schedparam;
	if ( (errno = posix_spawnattr_getschedparam(&attr, &schedparam)) )
		err(1, "posix_spawnattr_getschedparam");
	if ( (errno = posix_spawnattr_setschedparam(&attr, &schedparam)) )
		err(1, "posix_spawnattr_setschedparam");
	return 0;
}
