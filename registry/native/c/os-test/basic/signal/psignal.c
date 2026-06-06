/* Test whether a basic psignal invocation works. */

#include <stdio.h>
#include <signal.h>

#include "../basic.h"

int main(void)
{
	if ( !freopen("/dev/null", "w", stderr) )
		err(1, "freopen: /dev/null");
	psignal(SIGUSR1, "foo");
	if ( ferror(stderr) )
		exit(1);
	return 0;
}
