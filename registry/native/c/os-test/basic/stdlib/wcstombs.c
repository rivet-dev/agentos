/* Test whether a basic wcstombs invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char str[3];
	size_t amount = wcstombs(str, L"xy", sizeof(str));
	if ( amount == (size_t) -1 )
		err(1, "wcstombs");
	if ( amount != 2 )
		errx(1, "wcstombs did not return 2");
	if ( strcmp(str, "xy") != 0 )
		errx(1, "wcstombs did not provide \"xy\"");
	return 0;
}
