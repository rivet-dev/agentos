/* Test whether a basic wcscasecmp invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcscasecmp(L"foo", L"FOO") != 0 )
		errx(1, "wcscasecmp(\"foo\", \"FOO\") weren't equal");
	return 0;
}
