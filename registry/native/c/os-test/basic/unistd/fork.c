/* Test whether a basic fork invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	return pid ? 0 : 1;
}
