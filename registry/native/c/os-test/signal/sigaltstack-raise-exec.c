/* Test sigaltstack across exec from an signal with alternate stack. */

#include "signal.h"

char* self;

static void handler(int signum)
{
	(void) signum;
	execlp(self, self, "2", (char*) NULL);
	err(1, "execvl: %s", self);
}

int main(int argc, char* argv[])
{
	stack_t ss, old_ss;
	memset(&ss, 0, sizeof(ss));
	ss.ss_size = SIGSTKSZ;
	if ( !(ss.ss_sp = malloc(ss.ss_size)) )
		err(1, "malloc");
	sigaltstack(&ss, &old_ss);
	struct sigaction sa, old_sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = handler;
	sa.sa_flags = SA_ONSTACK;
	sigaction(SIGUSR1, &sa, &old_sa);
	self = argv[0];
	if ( argc == 1 )
		raise(SIGUSR1);
	printf("ss_sp%sNULL", old_ss.ss_sp ? "!=" : "==");
	if ( old_sa.sa_flags & SA_ONSTACK )
		printf(" SA_ONSTACK");
	if ( old_ss.ss_flags & SS_ONSTACK )
		printf(" SS_ONSTACK");
	if ( old_ss.ss_flags & SS_DISABLE )
		printf(" SS_DISABLE");
	printf("\n");
	return 0;
}
