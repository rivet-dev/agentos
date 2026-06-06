/* Test whether a basic strcoll_l invocation works. */

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
	int comparison = strcoll_l(a, b, locale);
	if ( comparison <= 0 )
		errx(1, "strcoll gave %d instead of non-negative", comparison);
	return 0;
}
