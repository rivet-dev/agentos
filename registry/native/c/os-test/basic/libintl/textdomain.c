/* Test whether a basic textdomain invocation works. */

#include <libintl.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	// Try default textdomain.
	const char* expected_default = "messages";
	char* output = textdomain(NULL);
	if ( !output )
		err(1, "first textdomain");
	if ( strcmp(output, expected_default) != 0 )
		errx(1, "default textdomain was not %s", expected_default);
	// Try setting a textdomain.
	const char* input = "os-test";
	output = textdomain(input);
	if ( !output )
		err(1, "second textdomain");
	if ( strcmp(output, input) != 0 )
		errx(1, "second textdomain did not return input");
	// Try getting the set textdomain.
	output = textdomain(NULL);
	if ( !output )
		err(1, "third textdomain");
	if ( strcmp(output, input) != 0 )
		errx(1, "third textdomain did not return the new textdomain");
	return 0;
}
