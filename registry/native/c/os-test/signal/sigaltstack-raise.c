/* Test sigaltstack. */

#include "signal.h"

static void handler(int signum)
{
	(void) signum;
	stack_t old_ss;
	sigaltstack(NULL, &old_ss);
	printf("ss_sp%sNULL", old_ss.ss_sp ? "!=" : "==");
	if ( old_ss.ss_flags & SS_ONSTACK )
		printf(" SS_ONSTACK");
	if ( old_ss.ss_flags & SS_DISABLE )
		printf(" SS_DISABLE");
	printf("\n");
	exit(0);
}

int main(void)
{
	stack_t ss;
	memset(&ss, 0, sizeof(ss));
	ss.ss_size = SIGSTKSZ;
	if ( !(ss.ss_sp = malloc(ss.ss_size)) )
		err(1, "malloc");
	sigaltstack(&ss, NULL);
	struct sigaction sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = handler;
	sa.sa_flags = SA_ONSTACK;
	sigaction(SIGUSR1, &sa, NULL);
	raise(SIGUSR1);
	return 0;
}
