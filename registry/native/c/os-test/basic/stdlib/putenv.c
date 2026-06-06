/*[XSI]*/
/* Test whether a basic putenv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char* envvar = "FOO=foo";
	if ( putenv(envvar) )
		err(1, "putenv");
	if ( getenv("FOO") != envvar + 4 )
		errx(1, "getenv did not return putenv's string");
	return 0;
}
