/* Test whether a basic wcsrtombs invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	char mbs[4] = "";
	const wchar_t* wcs = L"foo";
	const wchar_t* ptr = wcs;
	size_t amount = wcsrtombs(mbs, &ptr, 4, &ps);
	if ( amount != 3 )
		err(1, "wcsnrtombs() != 3");
	if ( strcmp(mbs, "foo") != 0 )
		errx(1, "did not encode \"foo\"");
	if ( ptr )
		errx(1, "wrong output pointer");
	return 0;
}
