/*[XSI]*/
/* Test whether a basic strptime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	const char* input = "2021-11-16 19:58:28";
	struct tm tm;
	char* result = strptime(input, "%Y-%m-%d %H:%M:%S", &tm);
	if ( !result )
		errx(1, "strptime returned NULL");
	if ( result != input + strlen(input) )
		errx(1, "strptime did not return a pointer to the end of input");
	char output[64];
	size_t length = strftime(output, sizeof(output), "%Y-%m-%d %H:%M:%S", &tm);
	if ( !length )
		err(1, "strftime");
	if ( strcmp(input, output) != 0 )
		errx(1, "strptime parsed %s not %s", output, input);
	return 0;
}
