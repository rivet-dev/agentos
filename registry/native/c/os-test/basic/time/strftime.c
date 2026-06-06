/* Test whether a basic strftime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	struct tm tm =
	{
		.tm_year = 121,
		.tm_mon = 10,
		.tm_mday = 16,
		.tm_wday = 2,
		.tm_hour = 19,
		.tm_min = 58,
		.tm_sec = 28,
	};
	char buf[64];
	size_t length = strftime(buf, sizeof(buf), "%Y-%m-%d %H:%M:%S", &tm);
	if ( !length )
		err(1, "strftime");
	const char* expected = "2021-11-16 19:58:28";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "strftime gave %s not %s", buf, expected);
	return 0;
}
