/*[SPN]*/
/* Test whether a basic posix_spawnattr_init invocation works. */

#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	return 0;
}
