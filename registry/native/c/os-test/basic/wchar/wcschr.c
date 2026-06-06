/* Test whether a basic wcschr invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t buf[] = L"abcdefg";
	if ( wcschr(buf, L'e') != buf + 4 )
		errx(1, "wcschr did not return e'");
	if ( wcschr(buf, L'x') )
		errx(1, "wcschr found absent character");
	return 0;
}
