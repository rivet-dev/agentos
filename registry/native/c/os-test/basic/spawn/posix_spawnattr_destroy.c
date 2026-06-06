/*[SPN]*/
/* Test whether a basic posix_spawnattr_destroy invocation works. */

#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	if ( (errno = posix_spawnattr_destroy(&attr)) )
		err(1, "posix_spawnattr_destroy");
	return 0;
}
