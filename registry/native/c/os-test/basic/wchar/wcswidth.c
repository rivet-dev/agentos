/*[XSI]*/
/* Test whether a basic wcswidth invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcswidth(L"foo", 2) != 2 )
		errx(1, "wcswidth(L\"foo\", 2) != 2");
	if ( wcswidth(L"foo", 4) != 3 )
		errx(1, "wcswidth(L\"foo\", 4) != 3");
	return 0;
}
