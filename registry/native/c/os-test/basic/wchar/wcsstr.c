/* Test whether a basic wcsstr invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	const wchar_t haystack[] = L"haystack";
	wchar_t* ptr = wcsstr(haystack, L"sta");
	if ( !ptr )
		errx(1, "wcsstr was NULL");
	if ( ptr != haystack + 3 )
		errx(1, "wcsstr found wrong needle");
	return 0;
}
