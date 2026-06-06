/* Test whether a basic wcscoll_l invocation works. */

#include <locale.h>
#include <wchar.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( !locale )
		err(1, "newlocale");
	wchar_t a[8] = L"abcdefg";
	wchar_t b[8] = L"abcdeFG";
	int comparison = wcscoll_l(a, b, locale);
	if ( comparison <= 0 )
		errx(1, "wcscoll gave %d instead of non-negative", comparison);
	return 0;
}
