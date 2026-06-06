/* Test whether a basic swscanf invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t world[6];
	int value;
	int ret = swscanf(L"hello world 42", L"hello %5ls %d", world, &value);
	if ( ret < 0 )
		err(1, "swscanf");
	if ( ret != 2 )
		errx(1, "swscanf did not return 2");
	if ( wcscmp(world, L"world") != 0 )
		errx(1, "swscanf gave '%ls' instead of '%ls'", world, L"world");
	if ( value != 42 )
		errx(1, "swscanf gave %d' instead of %d", value, 42);
	return 0;
}
