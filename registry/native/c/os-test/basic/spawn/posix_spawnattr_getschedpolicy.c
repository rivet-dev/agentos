/*[SPN PS]*/
/* Test whether a basic posix_spawnattr_getschedpolicy invocation works. */

#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	// The default schedpolicy is unspecified.
	int schedpolicy;
	if ( (errno = posix_spawnattr_getschedpolicy(&attr, &schedpolicy)) )
		err(1, "posix_spawnattr_getschedpolicy");
	if ( (errno = posix_spawnattr_setschedpolicy(&attr, schedpolicy)) )
		err(1, "posix_spawnattr_setschedpolicy");
	return 0;
}
