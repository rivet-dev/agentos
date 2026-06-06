/* Test whether a basic fexecve invocation works. */

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
#ifdef O_EXEC
	int fd = open("unistd/fexecve", O_EXEC);
#else
	int fd = open("unistd/fexecve", O_RDONLY);
#endif
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
	char* args[] = { "./fexecve", "success", (char*) NULL };
	fexecve(fd, args, environ);
	err(127, "fexecve: %s", args[0]);
}
