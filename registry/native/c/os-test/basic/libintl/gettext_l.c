/* Test whether a basic gettext_l invocation works. */

#include <locale.h>
#include <libintl.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	locale_t locale = duplocale(LC_GLOBAL_LOCALE);
	if ( locale == (locale_t) 0 )
		errx(1, "duplocale");
	// The gettext functions are required to return the input strings when in
	// the C or POSIX locales. Since we can't portably rely on the existence of
	// any other locale, these tests can only test the non-translation case.
	const char* input = "foo";
	const char* expected = "foo";
	char* output = gettext_l(input, locale);
	if ( !output )
		errx(1, "gettext_l returned NULL");
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" not \"%s\"", output, expected);
	if ( input != output )
		errx(1, "did not return input string");
	return 0;
}
