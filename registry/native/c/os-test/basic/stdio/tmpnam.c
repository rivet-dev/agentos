/*[OB]*/
/* Test whether a basic tmpnam invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	if ( !tmpnam(NULL) )
		err(1, "first tmpnam");
	char path[L_tmpnam];
	char* result = tmpnam(path);
	if ( !result )
		err(1, "second tmpnam");
	if ( result != path )
		errx(1, "tmpnam did not return the same pointer");
	return 0;
}
