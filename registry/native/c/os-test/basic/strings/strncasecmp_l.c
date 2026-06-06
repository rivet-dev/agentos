/* Test whether a basic strncasecmp_l invocation works. */

#include <locale.h>
#include <strings.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	if ( strncasecmp("foo", "FOX", 2) != 0 )
		errx(1, "strncasecmp(\"foo\", \"FOX\", 2) weren't equal", locale);
	return 0;
}
