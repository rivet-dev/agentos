/* Test whether a basic vwscanf invocation works. */

#include <stdarg.h>
#include <wchar.h>
#include <unistd.h>

#include "../basic.h"

static void indirect(const wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputws(L"hello world 42", stdout) == -1 )
		err(1, "puts");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	int ret = vwscanf(format, ap);
	if ( ret < 0 )
		err(1, "vwscanf");
	if ( ret != 2 )
		errx(1, "vwscanf did not return 2");
	va_end(ap);
}

int main(void)
{
	wchar_t world[6];
	int value;
	indirect(L"hello %5ls %d", world, &value);
	if ( wcscmp(world, L"world") != 0 )
		errx(1, "vswscanf gave '%ls' instead of '%ls'", world, L"world");
	if ( value != 42 )
		errx(1, "vswscanf gave %d' instead of %d", value, 42);
	return 0;
}
