/* Test whether a basic fwscanf invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputws(L"hello world 42", fp) == -1 )
		err(1, "fputws");
	errno = 0;
	rewind(fp);
	if ( errno )
		err(1, "rewind");
	wchar_t world[6];
	int value;
	int ret = fwscanf(fp, L"hello %5ls %d", world, &value);
	if ( ret < 0 )
		err(1, "fwscanf");
	if ( ret != 2 )
		errx(1, "fwscanf did not return 2");
	if ( wcscmp(world, L"world") != 0 )
		errx(1, "fwscanf gave '%ls' instead of '%ls'", world, L"world");
	if ( value != 42 )
		errx(1, "fwscanf gave %d' instead of %d", value, 42);
	return 0;
}
