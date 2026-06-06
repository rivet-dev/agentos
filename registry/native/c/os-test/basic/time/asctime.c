/*[OB]*/
/* Test whether a basic asctime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct tm tm =
	{
		.tm_year = 70,
		.tm_mon = 0,
		.tm_mday = 1,
		.tm_wday = 4,
		.tm_hour = 0,
		.tm_min = 0,
		.tm_sec = 0,
	};
	char* result = asctime(&tm);
	if ( !result )
		errx(1, "asctime returned NULL");
	const char* expected = "Thu Jan  1 00:00:00 1970\n";
	if ( strcmp(result, expected) != 0 )
		errx(1, "asctime gave '%s' expected '%s'\n", result, expected);
	return 0;
}
