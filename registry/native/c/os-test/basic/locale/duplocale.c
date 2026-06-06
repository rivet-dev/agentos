/* Test whether a basic duplocale invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	locale_t locale1 = duplocale(LC_GLOBAL_LOCALE);
	if ( locale1 == (locale_t) 0 )
		err(1, "duplocale(LC_GLOBAL_LOCALE)");
	locale_t locale2 = duplocale(locale1);
	if ( locale2 == (locale_t) 0 )
		err(1, "duplocale(locale1)");
	locale_t locale3 = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( locale3 == (locale_t) 0 )
		err(1, "newlocale");
	locale_t locale4 = duplocale(locale3);
	if ( locale4 == (locale_t) 0 )
		err(1, "duplocale(locale3)");
	freelocale(locale1);
	freelocale(locale2);
	freelocale(locale3);
	freelocale(locale4);
	return 0;
}
