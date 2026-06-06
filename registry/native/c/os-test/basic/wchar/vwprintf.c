/* Test whether a basic vwprintf invocation works. */

#include <stdarg.h>
#include <wchar.h>
#include <unistd.h>

#include "../basic.h"

static int indirect(wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( vwprintf(format, ap) < 0 )
		err(1, "vwprintf");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), stdin) )
		err(1, "fgetws");
	const wchar_t* expected = L"hello world 42";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "vwprintf wrote '%ls' instead of '%ls'", buf, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect(L"hello %ls %d", L"world", 42);
}
