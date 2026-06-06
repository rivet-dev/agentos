/* Test whether a basic execl invocation works. */

#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "execl invoked incorrectly");
		return 0;
	}
	execl("unistd/execl", "unistd/execl", "success", (char*) NULL);
	err(127, "execl: unistd/execl");
}
