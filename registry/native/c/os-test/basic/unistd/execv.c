/* Test whether a basic execv invocation works. */

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
	char* args[] = { "unistd/execv", "success", (char*) NULL };
	execv(args[0], args);
	err(127, "execv: %s", args[0]);
}
