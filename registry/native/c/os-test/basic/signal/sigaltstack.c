/*[XSI]*/
/* Test whether a basic sigaltstack invocation works. */

#include <signal.h>

#include "../basic.h"

static volatile sig_atomic_t received;

static void handler(int signo)
{
	received = signo;
	stack_t old_ss;
	if ( sigaltstack(NULL, &old_ss) < 0 )
		err(1, "signal handler sigaltstack");
	if ( !(old_ss.ss_flags & SS_ONSTACK) )
		errx(1, "not on signal alternate stack");
	if ( old_ss.ss_flags != SS_ONSTACK )
		printf("ss_flags != SS_ONSTACK");
}

int main(void)
{
	stack_t ss = { .ss_size = SIGSTKSZ }, old_ss;
	if ( !(ss.ss_sp = malloc(ss.ss_size)) )
		err(1, "malloc");
	if ( sigaltstack(&ss, &old_ss) < 0 )
		err(1, "sigaltstack");
	if ( old_ss.ss_flags != SS_DISABLE )
		errx(1, "old_ss.ss_flags != SS_DISABLE");
	struct sigaction sa = { .sa_handler = handler, .sa_flags = SA_ONSTACK };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	raise(SIGUSR1);
	if ( received != SIGUSR1 )
		err(1, "signal was not received");
	return 0;
}
