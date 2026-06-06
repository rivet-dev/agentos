/* Test whether a basic wmemmove invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t buf[8] = L"abcdefg";

	// Test forward wmemmove.
	void* ptr = wmemmove(buf + 1, buf, 4);
	if ( ptr != buf + 1 )
		errx(1, "forward wmemmove did not return dst");
	const wchar_t* expected = L"aabcdfg";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "forward wmemmove gave %ls instead of %ls", buf, expected);

	// Test backward wmemmove.
	ptr = wmemmove(buf + 3, buf + 4, 4);
	if ( ptr != buf + 3 )
		errx(1, "backward wmemmove did not return dst");
	expected = L"aabdfg";
	if ( wcscmp(buf, expected) != 0 )
		errx(1, "backward wmemmove gave %ls instead of %ls", buf, expected);
	return 0;
}
