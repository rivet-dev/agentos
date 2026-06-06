/* Test whether a basic wcsxfrm_l invocation works. */

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
	wchar_t A[8];
	wchar_t B[8];
	if ( wcsxfrm_l(A, a, sizeof(A) / sizeof(A[0]), locale) != 7 )
		errx(1, "wcsxfrm_l A did not return 7");
	if ( wcsxfrm_l(B, b, sizeof(B) / sizeof(B[0]), locale) != 7 )
		errx(1, "wcsxfrm_l B did not return 7");
	int cmp = wcscmp(A, B);
	int coll = wcscoll_l(a, b, locale);
	cmp = cmp < 0 ? -1 : cmp > 0 ? 1 : 0;
	coll = coll < 0 ? -1 : coll > 0 ? 1 : 0;
	if ( cmp != coll )
		errx(1, "wcscoll_l gave %d but wcscmp gave %d", cmp, coll);
	return 0;
}
