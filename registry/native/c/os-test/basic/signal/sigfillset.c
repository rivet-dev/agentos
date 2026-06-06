/* Test whether a basic sigfillset invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	if ( sigfillset(&set) < 0 )
		err(1, "sigemptyset");
	if ( sigismember(&set, SIGUSR1) != 1 )
		errx(1, "set was not filled");
	return 0;
}
