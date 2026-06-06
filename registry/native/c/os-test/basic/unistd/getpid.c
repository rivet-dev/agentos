/* Test whether a basic getpid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getpid() == (pid_t) -1 )
		err(1, "getpid");
	return 0;
}
