/* Test whether a basic execle invocation works. */

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "execle invoked incorrectly");
		if ( !getenv("OS_TEST_EXECLE") )
			errx(1, "$OS_TEST_EXECLE unset");
		return 0;
	}
	if ( setenv("OS_TEST_EXECLE", "set", 1) < 0 )
		err(1, "setenv");
	execle("unistd/execle", "unistd/execle", "success", (char*) NULL, environ);
	err(127, "execle: unistd/execle");
}
