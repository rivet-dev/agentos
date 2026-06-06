/* Test whether a basic faccessat invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int dir = open("..", O_RDONLY | O_DIRECTORY);
	if ( dir < 0 )
		err(1, "open: ..");
	if ( faccessat(dir, "basic/unistd/faccessat.c", F_OK, AT_EACCESS) < 0 )
		err(1, "faccessat");
	return 0;
}
