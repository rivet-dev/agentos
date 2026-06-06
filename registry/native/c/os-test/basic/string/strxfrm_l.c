/* Test whether a basic strxfrm_l invocation works. */

#include <locale.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( !locale )
		err(1, "newlocale");
	char a[8] = "abcdefg";
	char b[8] = "abcdeFG";
	char A[8];
	char B[8];
	if ( strxfrm_l(A, a, sizeof(A), locale) != 7 )
		errx(1, "strxfrm_l A did not return 7");
	if ( strxfrm_l(B, b, sizeof(B), locale) != 7 )
		errx(1, "strxfrm_l B did not return 7");
	int cmp = strcmp(A, B);
	int coll = strcoll_l(a, b, locale);
	cmp = cmp < 0 ? -1 : cmp > 0 ? 1 : 0;
	coll = coll < 0 ? -1 : coll > 0 ? 1 : 0;
	if ( cmp != coll )
		errx(1, "strcoll_l gave %d but strcmp gave %d", cmp, coll);
	return 0;
}
