/* Test whether a basic fwprintf invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fwprintf(fp, L"hello %ls %d", L"world", 42) < 0 )
		err(1, "fwprintf");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), fp) )
		err(1, "fgetws");
	const wchar_t* expected = L"hello world 42";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "fwprintf wrote '%ls' instead of '%ls'", buf, expected);
	return 0;
}
