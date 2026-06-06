/*[SPN]*/
/* Test whether a basic posix_spawnattr_getflags invocation works. */

#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	short flags;
	if ( (errno = posix_spawnattr_getflags(&attr, &flags)) )
		err(1, "posix_spawnattr_getflags");
	if ( flags != 0 )
		errx(1, "default flags was not 0");
	short new_flags = POSIX_SPAWN_SETPGROUP;
	if ( (errno = posix_spawnattr_setflags(&attr, new_flags)) )
		err(1, "posix_spawnattr_setflags");
	if ( (errno = posix_spawnattr_getflags(&attr, &flags)) )
		err(1, "posix_spawnattr_getflags");
	if ( flags != new_flags )
		errx(1, "new flags were not set");
	return 0;
}
