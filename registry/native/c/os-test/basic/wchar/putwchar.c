/* Test whether a basic putwchar invocation works. */

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
	if ( putwchar(L'x') == WEOF )
		err(1, "putwchar");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), stdin) )
		err(1, "fgetws");
	const wchar_t* expected = L"x";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "putwchar wrote '%ls' instead of '%ls'", buf, expected);
	return 0;
}
