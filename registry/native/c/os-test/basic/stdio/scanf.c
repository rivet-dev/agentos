/* Test whether a basic scanf invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputs("hello world 42", stdout) == EOF )
		err(1, "puts");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	char world[6];
	int value;
	int ret = scanf("hello %5s %d", world, &value);
	if ( ret < 0 )
		err(1, "sscanf");
	if ( ret != 2 )
		errx(1, "sscanf did not return 2");
	if ( strcmp(world, "world") != 0 )
		errx(1, "sscanf gave '%s' instead of '%s'", world, "world");
	if ( value != 42 )
		errx(1, "sscanf gave %d' instead of %d", value, 42);
	return 0;
}
