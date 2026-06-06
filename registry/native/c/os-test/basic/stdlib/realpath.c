/* Test whether a basic realpath invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* path = realpath(".", NULL);
	if ( !path )
		err(1, "realpath: .");
	if ( path[0] != '/' )
		errx(1, "path was not absolute");
	free(path);
	return 0;
}
