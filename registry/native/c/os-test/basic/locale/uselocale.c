/* Test whether a basic uselocale invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( locale == (locale_t) 0 )
		err(1, "newlocale");
	locale_t old_locale = uselocale(locale);
	if ( old_locale != LC_GLOBAL_LOCALE )
		errx(1, "uselocale(locale) did not return LC_GLOBAL_LOCALE");
	old_locale = uselocale((locale_t) 0);
	if ( old_locale != locale )
		errx(1, "uselocale((locale_t) did not return locale");
	return 0;
}
