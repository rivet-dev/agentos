/* Test whether a basic fchown invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fd = open(".", O_RDONLY);
	if ( fd < 0 )
		err(1, "open: .");
	if ( fchown(fd, (uid_t) -1, (gid_t) -1) < 0 && errno != EPERM )
		err(1, "fchown");
	return 0;
}
