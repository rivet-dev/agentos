/* Test whether a basic nl_langinfo_l invocation works. */

#include <locale.h>
#include <langinfo.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		err(1, "newlocale");
	char* output = nl_langinfo_l(MON_1, locale);
	const char* expected = "January";
	if ( !output )
		err(1, "nl_langinfo MON_1");
	if ( !output[0] )
		errx(1, "nl_langinfo MON_1 = \"\"");
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" instead of \"%s\"", output, expected);
	return 0;
}
