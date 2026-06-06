/* Test whether a basic ungetwc invocation works. */

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
	if ( ungetwc(L'X', fp) == WEOF )
		err(1, "ungetwc");
	wchar_t out[256];
	if ( !fgetws(out, sizeof(out)/sizeof(out[0]), fp) )
		err(1, "fgetws");
	const wchar_t* expected = L"Xfoo";
	if ( wcscmp(out, expected) != 0 )
		errx(1, "got '%ls' instead of '%ls'", out, expected);
	return 0;
}
