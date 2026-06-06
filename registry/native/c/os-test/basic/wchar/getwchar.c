/* Test whether a basic getwchar invocation works. */

#include <wchar.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputwc(L'x', stdout) == WEOF )
		err(1, "fputwc");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	wint_t c = getwchar();
	if ( c == WEOF )
		err(1, "getwchar");
	if ( c != L'x' )
		errx(1, "getwchar did not get 'x'");
	return 0;
}
