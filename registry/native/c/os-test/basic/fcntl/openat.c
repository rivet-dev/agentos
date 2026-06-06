/* Test whether a basic openat invocation works. */

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	int dirfd = openat(AT_FDCWD, "fcntl", O_RDONLY | O_DIRECTORY);
	if ( dirfd < 0 )
		err(1, "openat: fcntl");
	int fd = openat(dirfd, "openat", O_RDONLY);
	if ( fd < 0 )
		err(1, "openat: openat");
	return 0;
}
