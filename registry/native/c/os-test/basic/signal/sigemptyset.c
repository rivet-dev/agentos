/* Test whether a basic sigemptyset invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	if ( sigemptyset(&set) < 0 )
		err(1, "sigemptyset");
	if ( sigismember(&set, SIGUSR1) != 0 )
		errx(1, "set was not empty");
	return 0;
}
