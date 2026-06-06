/* Test handling SIGCHLD and what happens after exec. */

#include "signal.h"

#if defined(__minix__)
#ifndef SA_SIGINFO
#define SA_SIGINFO 0
#endif
#endif

static void handler(int signum)
{
	(void) signum;
	int errnum = errno;
	printf("SIGUSR1\n");
	fflush(stdout);
	errno = errnum;
}

int main(int argc, char* argv[])
{
	struct sigaction sa, old_sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = handler;
	sa.sa_flags = SA_RESETHAND | SA_RESTART | SA_SIGINFO | SA_NODEFER;
	sigaction(SIGUSR1, &sa, &old_sa);
	if ( argc == 1 && execlp(argv[0], argv[0], "2", (char*) NULL) < 0 )
		err(1, "execvl: %s", argv[0]);
	printf("0");
	if ( old_sa.sa_flags & SA_RESETHAND )
		printf(" | SA_RESETHAND");
	if ( old_sa.sa_flags & SA_RESTART )
		printf(" | SA_RESTART");
	if ( old_sa.sa_flags & SA_SIGINFO )
		printf(" | SA_SIGINFO");
	if ( old_sa.sa_flags & SA_NODEFER )
		printf(" | SA_NODEFER");
	printf("\n");
	return 0;
}
