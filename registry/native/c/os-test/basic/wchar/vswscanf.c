/* Test whether a basic vswscanf invocation works. */

#include <stdarg.h>
#include <wchar.h>

#include "../basic.h"

static void indirect(const wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vswscanf(L"hello world 42", format, ap);
	if ( ret < 0 )
		err(1, "vswscanf");
	if ( ret != 2 )
		errx(1, "vswscanf did not return 2");
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
