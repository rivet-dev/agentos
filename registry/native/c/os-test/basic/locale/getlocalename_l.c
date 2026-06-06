/* Test whether a basic getlocalename_l invocation works. */

#include <locale.h>

#include "../basic.h"

int main(void)
{
	const char* name = getlocalename_l(LC_COLLATE, LC_GLOBAL_LOCALE);
	if ( !name )
		errx(1, "getlocalename_l(LC_GLOBAL_LOCALE) returned NULL");
	if ( !name[0] )
		errx(1, "getlocalename_l(LC_GLOBAL_LOCALE) returned empty string");
	locale_t locale = newlocale(LC_COLLATE_MASK, "C", (locale_t) 0);
	if ( locale == (locale_t) 0 )
		err(1, "newlocale");
	name = getlocalename_l(LC_COLLATE, locale);
	if ( !name )
		errx(1, "getlocalename_l(locale) returned NULL");
	if ( !name[0] )
		errx(1, "getlocalename_l(locale) returned empty string");
	return 0;
}
