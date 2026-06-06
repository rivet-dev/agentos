/* Test whether a basic posix_close invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( posix_close(0, POSIX_CLOSE_RESTART) < 0 )
		err(1, "posix_close");
	return 0;
}
