/* Test whether a basic strcasecmp_l invocation works. */

#include <locale.h>
#include <strings.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	if ( strcasecmp_l("foo", "FOO", locale) != 0 )
		errx(1, "strcasecmp(\"foo\", \"FOO\") weren't equal");
	return 0;
}
