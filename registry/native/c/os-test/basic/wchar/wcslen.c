/* Test whether a basic wcslen invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcslen(L"foo") != 3 )
		errx(1, "wcslen did not return 3");
	return 0;
}
