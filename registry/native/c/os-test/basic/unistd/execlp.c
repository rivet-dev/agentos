/* Test whether a basic execlp invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "execle invoked incorrectly");
		return 0;
	}
	if ( setenv("PATH", "unistd", 1) < 0 )
		err(1, "setenv");
	execlp("execlp", "execlp", "success", (char*) NULL);
	err(127, "execlp: execlp");
	return 0;
}
