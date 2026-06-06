/* Test whether a basic psiginfo invocation works. */

#include <stdio.h>
#include <signal.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	siginfo_t info =
	{
		.si_signo = SIGUSR1,
		.si_code = SI_USER,
		.si_pid = getpid(),
		.si_uid = getuid(),
	};
	if ( !freopen("/dev/null", "w", stderr) )
		err(1, "freopen: /dev/null");
	psiginfo(&info, "foo");
	if ( ferror(stderr) )
		exit(1);
	return 0;
}
