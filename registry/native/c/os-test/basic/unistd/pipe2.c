/* Test whether a basic pipe2 invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe2(fds, O_CLOEXEC) < 0 )
		err(1, "pipe2");
	return 0;
}
