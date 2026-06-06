/* Test whether a basic sscanf invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char world[6];
	int value;
	int ret = sscanf("hello world 42", "hello %5s %d", world, &value);
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
