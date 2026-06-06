/* Test whether a basic wcscspn invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"abcdefg";
	if ( wcscspn(buf, L"eg") != 4 )
		errx(1, "wcscspn did not find e'");
	if ( wcscspn(buf, L"x") != 7 )
		errx(1, "wcscspn found absent character");
	return 0;
}
