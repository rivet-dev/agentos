/* Test ignoring SIGUSR1 and what happens after exec. */

#include "signal.h"

int main(int argc, char* argv[])
{
	void (*old)(int) = signal(SIGUSR1, SIG_IGN);
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
