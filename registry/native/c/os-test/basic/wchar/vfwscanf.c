/* Test whether a basic vfwscanf invocation works. */

#include <stdarg.h>
#include <wchar.h>

#include "../basic.h"

static void indirect(const wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputws(L"hello world 42", fp) == -1 )
		err(1, "fputws");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	int ret = vfwscanf(fp, format, ap);
	if ( ret < 0 )
		err(1, "vfwscanf");
	if ( ret != 2 )
		errx(1, "vfwscanf did not return 2");
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
