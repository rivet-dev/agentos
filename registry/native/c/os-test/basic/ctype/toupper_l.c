/* Test whether a basic toupper_l invocation works. */

#include <ctype.h>
#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	char c1 = 'x';
	char c2 = 'X';
	char c3 = toupper_l(c1, locale);
	if ( c3 != c2 )
		errx(1, "toupper_l('%c') was not '%c'", c1, c2);
	return 0;
}
