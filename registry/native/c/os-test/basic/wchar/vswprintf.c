/* Test whether a basic vswprintf invocation works. */

#include <stdarg.h>
#include <wchar.h>

#include "../basic.h"

#pragma GCC diagnostic ignored "-Wformat-truncation"

static int indirect(const wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	wchar_t buffer[15];
	int ret = vswprintf(buffer, sizeof(buffer)/sizeof(buffer[0]), format, ap);
	if ( ret < 0 )
		err(1, "vswprintf");
	if ( (size_t) ret != wcslen(L"hello world 42") )
		err(1, "vswprintf returned wrong length");
	const wchar_t* expected = L"hello world 42";
	if ( wcscmp(buffer, expected) != 0 )
		err(1, "vswprintf gave '%ls' instead of '%ls'", buffer, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect(L"hello %ls %d", L"world", 42);
}
