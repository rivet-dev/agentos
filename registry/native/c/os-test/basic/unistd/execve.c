/* Test whether a basic execve invocation works. */

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
			err(1, "execv invoked incorrectly");
		if ( !getenv("OS_TEST_EXECVE") )
			errx(1, "$OS_TEST_EXECVE unset");
		return 0;
	}
	if ( setenv("OS_TEST_EXECVE", "set", 1) < 0 )
		err(1, "setenv");
	char* args[] = { "unistd/execve", "success", (char*) NULL };
	execve(args[0], args, environ);
	err(127, "execve: %s", args[0]);
	return 0;
}
