/* Test whether a basic getpgid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getpgid(0) == (pid_t) -1 )
		err(1, "getpgid");
	return 0;
}
