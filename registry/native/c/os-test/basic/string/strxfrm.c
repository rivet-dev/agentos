/* Test whether a basic strxfrm invocation works. */


#include <string.h>

#include "../basic.h"

int main(void)
{
	char a[8] = "abcdefg";
	char b[8] = "abcdeFG";
	char A[8];
	char B[8];
	if ( strxfrm(A, a, sizeof(A)) != 7 )
		errx(1, "strxfrm A did not return 7");
	if ( strxfrm(B, b, sizeof(B)) != 7 )
		errx(1, "strxfrm B did not return 7");
	int cmp = strcmp(A, B);
	int coll = strcoll(a, b);
	cmp = cmp < 0 ? -1 : cmp > 0 ? 1 : 0;
	coll = coll < 0 ? -1 : coll > 0 ? 1 : 0;
	if ( cmp != coll )
		errx(1, "strcoll gave %d but strcmp gave %d", cmp, coll);
	return 0;
}
