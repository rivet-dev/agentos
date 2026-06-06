/* Test whether a basic vfwprintf invocation works. */

#include <wchar.h>

#include "../basic.h"

static int indirect(wchar_t* format, ...)
{
	va_list ap;
	va_start(ap, format);
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( vfwprintf(fp, format, ap) < 0 )
		err(1, "vfwprintf");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), fp) )
		err(1, "fgetws");
	const wchar_t* expected = L"hello world 42";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "vfwprintf wrote '%ls' instead of '%ls'", buf, expected);
	va_end(ap);
	return 0;
}

int main(void)
{
	return indirect(L"hello %ls %d", L"world", 42);
}
