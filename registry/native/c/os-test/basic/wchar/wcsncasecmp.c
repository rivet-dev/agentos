/* Test whether a basic wcsncasecmp invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcsncasecmp(L"foo", L"FOX", 2) != 0 )
		errx(1, "wcsncasecmp(\"foo\", \"FOX\", 2) weren't equal");
	return 0;
}
