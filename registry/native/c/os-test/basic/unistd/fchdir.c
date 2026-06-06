/* Test whether a basic fchdir invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int dir = open("..", O_RDONLY | O_DIRECTORY);
	if ( dir < 0 )
		err(1, "open: ..");
	if ( fchdir(dir) < 0 )
		err(1, "fchdir");
	return 0;
}
