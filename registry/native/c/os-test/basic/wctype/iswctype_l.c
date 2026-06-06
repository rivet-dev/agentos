/* Test whether a basic iswctype_l invocation works. */

#include <locale.h>
#include <wctype.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	wctype_t charclass = wctype_l("alpha", locale);
	if ( !charclass )
		errx(1, "wctype failed");
	wchar_t wc1 = L'a';
	wchar_t wc2 = L'1';
	if ( !iswctype_l(wc1, charclass, locale) )
		errx(1, "iswctype_l('%lc', charclass) was not true", wc1);
	if ( iswctype_l(wc2, charclass, locale) )
		errx(1, "iswctype_l('%lc', charclass) was not false", wc2);
	// "If charclass is (wctype_t)0, the iswctype() and iswctype_l() functions
	//  shall return 0."
	if ( iswctype_l(wc1, (wctype_t) 0, locale) )
		errx(1, "iswctype_l('%lc', (wctype_t) 0) was not false", wc1);
	if ( iswctype_l(wc2, (wctype_t) 0, locale) )
		errx(1, "iswctype_l('%lc', (wctype_t) 0) was not false", wc2);
	return 0;
}
