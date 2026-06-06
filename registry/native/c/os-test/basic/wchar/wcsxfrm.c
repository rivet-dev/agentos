/* Test whether a basic wcsxfrm invocation works. */


#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t a[8] = L"abcdefg";
	wchar_t b[8] = L"abcdeFG";
	wchar_t A[8];
	wchar_t B[8];
	if ( wcsxfrm(A, a, sizeof(A) / sizeof(A[0])) != 7 )
		errx(1, "wcsxfrm A did not return 7");
	if ( wcsxfrm(B, b, sizeof(A) / sizeof(B[0])) != 7 )
		errx(1, "wcsxfrm B did not return 7");
	int cmp = wcscmp(A, B);
	int coll = wcscoll(a, b);
	cmp = cmp < 0 ? -1 : cmp > 0 ? 1 : 0;
	coll = coll < 0 ? -1 : coll > 0 ? 1 : 0;
	if ( cmp != coll )
		errx(1, "wcscoll gave %d but wcscmp gave %d", cmp, coll);
	return 0;
}
