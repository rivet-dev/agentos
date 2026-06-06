/* Test whether a basic fcntl invocation works. */

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	// See if FD_CLOEXEC can be set.
	if ( fcntl(1, F_SETFD, 0) < 0 )
		err(1, "fcntl(F_SETFD)");
	int ret = fcntl(1, F_GETFD, 0);
	if ( ret < 0 )
		err(1, "fcntl(F_GETFD)");
	if ( ret != 0 )
		errx(1, "fcntl(F_GETFD) != 0");

	// See if FD_CLOEXEC can be unset.
	if ( fcntl(1, F_SETFD, FD_CLOEXEC) < 0 )
		err(1, "fcntl(F_SETFD)");
	ret = fcntl(1, F_GETFD, 0);
	if ( ret < 0 )
		err(1, "fcntl(F_GETFD)");
	if ( ret != FD_CLOEXEC )
		errx(1, "fcntl(F_GETFD) != FD_CLOEXEC");
	return 0;
}
