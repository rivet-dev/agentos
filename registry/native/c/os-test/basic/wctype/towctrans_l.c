/* Test whether a basic towctrans_l invocation works. */

#include <locale.h>
#include <wctype.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	wctrans_t desc = wctrans_l("tolower", locale);
	if ( !desc )
		err(1, "wctrans_l");
	wchar_t wc1 = L'X';
	wchar_t wc2 = L'x';
	wchar_t wc3 = towctrans_l(wc1, desc, locale);
	if ( wc3 != wc2 )
		errx(1, "wctrans_l('%lc', desc) was not '%lc'", wc1, wc2);
	return 0;
}
