/* Test whether a basic wctype_l invocation works. */

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
		errx(1, "wctype_l failed");
	return 0;
}
