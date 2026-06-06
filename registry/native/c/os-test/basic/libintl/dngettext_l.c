/* Test whether a basic dngettext_l invocation works. */

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
	const char* input_plural = "foos";
	// Test 0.
	char* output0 = dngettext_l("os-test", input, input_plural, 0, locale);
	if ( !output0 )
		errx(1, "dngettext_l 0 returned NULL");
	if ( strcmp(output0, input_plural) != 0 )
		errx(1, "0 got \"%s\" not \"%s\"", output0, input_plural);
	if ( output0 != input_plural )
		errx(1, "0 did not return input string");
	// Test 1.
	char* output1 = dngettext_l("os-test", input, input_plural, 1, locale);
	if ( !output1 )
		errx(1, "dngettext_l 1 returned NULL");
	if ( strcmp(output1, input) != 0 )
		errx(1, "1 got \"%s\" not \"%s\"", output1, input);
	if ( output1 != input )
		errx(1, "1 did not return input string");
	// Test 2.
	char* output2 = dngettext_l("os-test", input, input_plural, 2, locale);
	if ( !output2 )
		errx(1, "dngettext_l 2 returned NULL");
	if ( strcmp(output2, input_plural) != 0 )
		errx(1, "2 got \"%s\" not \"%s\"", output2, input_plural);
	if ( output2 != input_plural )
		errx(1, "2 did not return input string");
	// Test 9.
	char* output9 = dngettext_l("os-test", input, input_plural, 9, locale);
	if ( !output9 )
		errx(1, "dngettext_l 9 returned NULL");
	if ( strcmp(output9, input_plural) != 0 )
		errx(1, "9 got \"%s\" not \"%s\"", output9, input_plural);
	if ( output9 != input_plural )
		errx(1, "9 did not return input string");
	return 0;
}
