/* Test whether a basic getchar_unlocked invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputc('x', stdout) == EOF )
		err(1, "puts");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	flockfile(stdin);
	int c = getchar_unlocked();
	funlockfile(stdin);
	if ( c < 0 )
		err(1, "getchar_unlocked");
	if ( c != 'x' )
		errx(1, "getchar_unlocked did not get 'x'");
	return 0;
}
