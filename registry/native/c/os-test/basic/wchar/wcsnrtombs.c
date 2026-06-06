/* Test whether a basic wcsnrtombs invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	mbstate_t ps = {0};
	char mbs[4] = "";
	const wchar_t* wcs = L"foo";
	const wchar_t* ptr = wcs;
	size_t amount = wcsnrtombs(mbs, &ptr, 2, 4, &ps);
	if ( amount != 2 )
		err(1, "wcsnrtombs() != 2");
	if ( strncmp(mbs, "fo", 2) != 0 )
		errx(1, "did not encode \"fo\"");
	if ( ptr != wcs + 2 )
		errx(1, "wrong output pointer");
	return 0;
}
