/* Test whether a basic wcsnlen invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( wcsnlen(L"foo", 2) != 2 )
		errx(1, "wcsnlen did not return 2");
	return 0;
}
