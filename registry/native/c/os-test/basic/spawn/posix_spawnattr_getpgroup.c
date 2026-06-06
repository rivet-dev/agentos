/*[SPN]*/
/* Test whether a basic posix_spawnattr_getpgroup invocation works. */

#include <spawn.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	pid_t pgroup;
	if ( (errno = posix_spawnattr_getpgroup(&attr, &pgroup)) )
		err(1, "posix_spawnattr_getpgroup");
	if ( pgroup != 0 )
		errx(1, "default pgroup was not 0");
	pid_t new_pgroup = getpgid(0);
	if ( (errno = posix_spawnattr_setpgroup(&attr, new_pgroup)) )
		err(1, "posix_spawnattr_setpgroup");
	if ( (errno = posix_spawnattr_getpgroup(&attr, &pgroup)) )
		err(1, "posix_spawnattr_getpgroup");
	if ( pgroup != new_pgroup )
		errx(1, "new pgroup was not set");
	return 0;
}
