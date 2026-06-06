/* Test whether a basic fscanf invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char data[] = "hello world 42";
	FILE* fp = fmemopen(data, sizeof(data), "r");
	if ( !fp )
		err(1, "fmemopen");
	char world[6];
	int value;
	int ret = fscanf(fp, "hello %5s %d", world, &value);
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
