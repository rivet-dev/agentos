/* Test whether a basic system invocation works. */

#include <sys/wait.h>

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int status = system("exit 42");
	if ( status < 0 )
		err(1, "system");
	if ( !WIFEXITED(status) )
		errx(1, "sh -c 'exit 42' did not exit cleanly");
	if ( WEXITSTATUS(status) != 42 )
		errx(1, "sh -c 'exit 42' exited %d", WEXITSTATUS(status));
	return 0;
}
