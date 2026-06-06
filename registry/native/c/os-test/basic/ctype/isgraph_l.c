/* Test whether a basic isgraph_l invocation works. */

#include <ctype.h>
#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	char c1 = 'x';
	char c2 = ' ';
	if ( !isgraph_l(c1, locale) )
		errx(1, "isgraph_l('%c') was not true", c1);
	if ( isgraph_l(c2, locale) )
		errx(1, "isgraph_l('%c') was not false", c2);
	return 0;
}
