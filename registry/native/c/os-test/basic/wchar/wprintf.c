/* Test whether a basic wprintf invocation works. */

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
	if ( wprintf(L"hello %ls %d", L"world", 42) < 0 )
		err(1, "wprintf");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), stdin) )
		err(1, "fgetws");
	const wchar_t* expected = L"hello world 42";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "wprintf wrote '%ls' instead of '%ls'", buf, expected);
	return 0;
}
