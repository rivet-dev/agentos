/* Test whether a basic iswpunct_l invocation works. */

#include <locale.h>
#include <wctype.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	wchar_t wc1 = L'.';
	wchar_t wc2 = L'A';
	if ( !iswpunct_l(wc1, locale) )
		errx(1, "iswpunct_l('%lc') was not true", wc1);
	if ( iswpunct_l(wc2, locale) )
		errx(1, "iswpunct_l('%lc') was not false", wc2);
	return 0;
}
