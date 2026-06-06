/* Test whether a basic perror invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	if ( !freopen("/dev/null", "w", stderr) )
		err(1, "freopen: /dev/null");
	errno = EINVAL;
	perror("foo");
	if ( ferror(stderr) )
		exit(1);
	return 0;
}
