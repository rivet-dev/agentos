/* Test whether a basic pipe invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	return 0;
}
