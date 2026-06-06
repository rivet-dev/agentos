/* Test whether a basic wcsdup invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t* src = L"foo";
	wchar_t* dst = wcsdup(src);
	if ( !dst )
		err(1, "malloc");
	if ( wcscmp(src, dst) != 0 )
		err(1, "wcsdup gave %ls instead of %ls", src, dst);
	return 0;
}
