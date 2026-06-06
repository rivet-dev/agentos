/* Test whether a basic towupper_l invocation works. */

#include <locale.h>
#include <wctype.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	wchar_t wc1 = L'x';
	wchar_t wc2 = L'X';
	wchar_t wc3 = towupper_l(wc1, locale);
	if ( wc3 != wc2 )
		errx(1, "towupper_l('%lc') was not '%lc'", wc1, wc2);
	return 0;
}
