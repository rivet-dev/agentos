/* Test whether a basic fputws invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputws(L"foo", fp) == -1 )
		err(1, "fputws");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	wchar_t buf[256];
	if ( !fgetws(buf, sizeof(buf)/sizeof(buf[0]), fp) )
		err(1, "fgetws");
	const wchar_t* expected = L"foo";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "fputws wrote '%ls' instead of '%ls'", buf, expected);
	return 0;
}
