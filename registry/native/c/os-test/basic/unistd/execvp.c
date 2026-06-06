/* Test whether a basic execvp invocation works. */

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "execv invoked incorrectly");
		return 0;
	}
	if ( setenv("PATH", "unistd", 1) < 0 )
		err(1, "setenv");
	char* args[] = { "execvp", "success", (char*) NULL };
	execvp(args[0], args);
	err(127, "execvp: %s", args[0]);
	return 0;
}
