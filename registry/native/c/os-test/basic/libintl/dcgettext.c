/* Test whether a basic dcgettext invocation works. */

#include <locale.h>
#include <libintl.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	// The gettext functions are required to return the input strings when in
	// the C or POSIX locales. Since we can't portably rely on the existence of
	// any other locale, these tests can only test the non-translation case.
	const char* input = "foo";
	const char* expected = "foo";
	char* output = dcgettext("os-test", input, LC_NUMERIC);
	if ( !output )
		errx(1, "dcgettext returned NULL");
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" not \"%s\"", output, expected);
	if ( input != output )
		errx(1, "did not return input string");
	return 0;
}
