/* Test whether a basic fchownat invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int dir = open("..", O_RDONLY | O_DIRECTORY);
	if ( dir < 0 )
		err(1, "open: ..");
	if ( fchownat(dir, "basic/unistd/fchownat.c", (uid_t) -1, (gid_t) -1,
	              AT_SYMLINK_NOFOLLOW) < 0 && errno != EPERM )
		err(1, "fchownat");
	return 0;
}
