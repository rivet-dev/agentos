/* Test whether a basic regerror invocation works. */

#include <regex.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int ret;
	regex_t re;
	const char* regex = "(";
	if ( !(ret = regcomp(&re, regex, REG_EXTENDED)) )
		errx(1, "regcomp did not fail");
	// Get detailed error message.
	size_t needed = regerror(ret, &re, NULL, 0);
	if ( !needed )
		errx(1, "regerror returned 0");
	char* buffer = malloc(needed);
	if ( !buffer )
		err(1, "malloc");
	size_t produced = regerror(ret, &re, buffer, needed);
	if ( needed < produced)
		errx(1, "regerror asked for too small buffer");
	if ( !buffer[0] )
		errx(1, "regerror gave empty error");
	free(buffer);
	// Get rough error message (pass NULL regular expression).
	needed = regerror(ret, NULL, NULL, 0);
	if ( !needed )
		errx(1, "second regerror returned 0");
	buffer = malloc(needed);
	if ( !buffer )
		err(1, "malloc");
	produced = regerror(ret, NULL, buffer, needed);
	if ( needed < produced)
		errx(1, "second regerror asked for too small buffer");
	if ( !buffer[0] )
		errx(1, "second regerror gave empty error");
	free(buffer);
	return 0;
}
