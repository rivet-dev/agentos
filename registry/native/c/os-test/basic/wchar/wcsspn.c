/* Test whether a basic wcsspn invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"abcdefg";
	if ( wcsspn(buf, L"abcdf") != 4 )
		errx(1, "wcsspn did not find 'e'");
	if ( wcsspn(buf, L"abcdefg") != 7 )
		errx(1, "wcsspn found other character");
	return 0;
}
