/* Test whether a basic isalnum_l invocation works. */

#include <ctype.h>
#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	char c1 = 'a';
	char c2 = '@';
	if ( !isalnum_l(c1, locale) )
		errx(1, "isalnum_l('%c') was not true", c1);
	if ( isalnum_l(c2, locale) )
		errx(1, "isalnum_l('%c') was not false", c2);
	return 0;
}
