/* Test handling SIGCHLD and what happens after exec. */

#include "signal.h"

static void handler(int signum)
{
	(void) signum;
	int errnum = errno;
	printf("SIGCHLD\n");
	fflush(stdout);
	errno = errnum;
}

int main(int argc, char* argv[])
{
	void (*old)(int) = signal(SIGCHLD, handler);
	if ( argc == 1 && execlp(argv[0], argv[0], "2", (char*) NULL) < 0 )
		err(1, "execvl: %s", argv[0]);
	if ( old == SIG_IGN )
		printf("SIG_IGN\n");
	else if ( old == SIG_DFL )
		printf("SIG_DFL\n");
	else
		printf("handled\n");
	return 0;
}
