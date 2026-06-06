/* Test whether a basic str2sig invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	int signo;
	if ( str2sig("USR1", &signo) < 0 )
		err(1, "str2sig");
	if ( signo != SIGUSR1 )
		errx(1, "str2sig gave %d rather than SIGUSR1", signo);
	return 0;
}
