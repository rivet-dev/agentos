/* Test whether a basic fdopen invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	FILE* in = fdopen(fds[0], "r");
	if ( !in )
		err(1, "fdopen in");
	FILE* out = fdopen(fds[1], "w");
	if ( !out )
		err(1, "fdopen out");
	if ( fputc('x', out) == EOF )
		err(1, "fputc");
	if ( fflush(out) == EOF )
		err(1, "fflush");
	int c = fgetc(in);
	if ( c == EOF )
	{
		if ( feof(in) )
			errx(1, "fgetc: EOF");
		err(1, "fgetc");
	}
	if ( c != 'x' )
		errx(1, "fgetc did not return x");
	return 0;
}
