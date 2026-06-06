/* Test whether a basic strerror_l invocation works. */

#include <locale.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = newlocale(LC_ALL_MASK, "C", (locale_t) 0);
	if ( !locale )
		err(1, "newlocale");
	if ( !strerror_l(EILSEQ, locale) )
		errx(1, "strerror_l returned NULL");
	return 0;
}
