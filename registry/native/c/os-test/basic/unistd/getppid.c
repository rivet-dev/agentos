/* Test whether a basic getppid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getppid() == (pid_t) -1 )
		err(1, "getppid");
	return 0;
}
