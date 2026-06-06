/* Test whether a basic getpgrp invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getpgrp() == (pid_t) -1 )
		err(1, "getpgrp");
	return 0;
}
